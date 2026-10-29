import { ApiError, hex } from "../util";
import { MAX_BYTES } from "./protocol";

/** R2 validates the SHA-256 while the bounded stream is consumed. No complete
 * 50-MiB snapshot (or second concatenated copy) is buffered in the Worker. */
export async function uploadCiphertext(bucket: R2Bucket, key: string, body: ReadableStream<Uint8Array>, length: number, checksum: string) {
  // Node's synthetic test binding has no FixedLengthStream; the explicit counter
  // below enforces the same bounds there. workerd gets known-length R2 input.
  const fixed = typeof FixedLengthStream === "function" ? new FixedLengthStream(length) : new TransformStream<Uint8Array, Uint8Array>();
  const reader = body.getReader(), writer = fixed.writable.getWriter();
  let pumpError: unknown;
  const pump = (async () => {
    let size = 0;
    try {
      for (;;) {
        const part = await reader.read();
        if (part.done) break;
        size += part.value.byteLength;
        if (size > length || size > MAX_BYTES) throw new ApiError(413, "too_large", "The upload exceeds its reserved size.");
        await writer.write(part.value);
      }
      if (size !== length) throw new ApiError(400, "size_mismatch", "The encrypted upload is incomplete.");
      await writer.close();
    } catch (error) {
      pumpError = error;
      await writer.abort(error).catch(() => {});
      throw error;
    }
  })();
  // Attach a handler immediately; a failed reader can otherwise reject before
  // R2's promise settles and produces an unhandled rejection in the Worker.
  void pump.catch(() => {});
  const stop = async (reason?: unknown) => {
    // A conditional R2 create can return without consuming fixed.readable.
    // Cancelling that side releases a backpressured writer.write; aborting only
    // the writer would wait forever for the unconsumed chunk to be read.
    await Promise.allSettled([reader.cancel(reason), fixed.readable.cancel(reason), writer.abort(reason)]);
    await pump.catch(() => {});
  };
  try {
    let result = await bucket.put(key, fixed.readable, { sha256: checksum, onlyIf: new Headers({ "If-None-Match": "*" }) });
    if (!result) {
      // Another identical retry already wrote this immutable reserved key. R2
      // need not consume our body when the conditional create fails.
      await stop();
      result = await bucket.head(key);
    } else await pump;
    if (!result || result.size !== length || !result.checksums.sha256 || hex(result.checksums.sha256) !== checksum)
      throw new ApiError(503, "staging_integrity", "The staged object's verified metadata differs. Retry without changing the manifest.");
  } catch (error) {
    await stop(error);
    if (pumpError instanceof ApiError) throw pumpError;
    if (error instanceof ApiError) throw error;
    if (/checksum|BadDigest|SHA.?256/i.test(String(error))) throw new ApiError(400, "checksum_mismatch", "The encrypted payload does not match its immutable manifest.");
    throw error;
  } finally {
    reader.releaseLock(); writer.releaseLock();
  }
}
