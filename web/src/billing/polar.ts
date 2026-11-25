// Polar (polar.sh), the merchant of record: hosted checkout, the customer portal and signed
// webhooks (Standard Webhooks). https://polar.sh/docs
import type { Env } from "../env";
import { ApiError, base64url, fromBase64url, sameString, utf8 } from "../util";
import type { BillingEvent, BillingProvider, Metrics } from "./provider";

const apiBase = (env: Env) => (env.BILLING_ENV === "production" ? "https://api.polar.sh" : "https://sandbox-api.polar.sh");

function polarFetch(env: Env, method: string, path: string, body?: unknown): Promise<Response> {
  if (!env.POLAR_ACCESS_TOKEN) throw new ApiError(503, "not_configured", "Checkout isn't available yet.");
  return fetch(apiBase(env) + path, {
    method,
    headers: { Authorization: `Bearer ${env.POLAR_ACCESS_TOKEN}`, "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
}

const unavailable = async (method: string, path: string, response: Response) => {
  console.error(`[polar] ${method} ${path} → ${response.status}: ${await response.text()}`);
  return new ApiError(502, "billing_unavailable", "The payment service couldn't be reached. Try again in a minute.");
};

async function polarApi<T>(env: Env, method: string, path: string, body?: unknown): Promise<T> {
  const response = await polarFetch(env, method, path, body);
  if (!response.ok) throw await unavailable(method, path, response);
  return (response.status === 204 ? null : await response.json()) as T;
}

interface PolarMetrics {
  periods: ({ timestamp: string } & Record<string, number | null>)[];
  totals: Record<string, number | null>;
  metrics: Record<string, { slug: string; display_name: string; type: string } | null>;
}

/** A Standard Webhooks signature check, allowing five minutes of clock skew. */
export async function verifyWebhook(secret: string, headers: Headers, body: string, at = Date.now()): Promise<boolean> {
  const id = headers.get("webhook-id");
  const timestamp = headers.get("webhook-timestamp");
  const signatures = (headers.get("webhook-signature") ?? "")
    .split(" ")
    .filter((s) => s.startsWith("v1,"))
    .map((s) => s.slice(3));
  if (!id || !timestamp || !signatures.length || Math.abs(at / 1000 - Number(timestamp)) > 300) return false;
  // Secrets made since September 2026 are standard (base64 after `whsec_`); older ones are used
  // as their UTF-8 bytes. Polar's own SDK tries both, and so does this.
  const keys: Uint8Array[] = [utf8(secret)];
  try {
    keys.unshift(fromBase64url(secret.replace(/^whsec_/, "").replace(/\+/g, "-").replace(/\//g, "_")));
  } catch {
    /* not base64: only the legacy key applies */
  }
  for (const raw of keys) {
    const key = await crypto.subtle.importKey("raw", raw, { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
    const mac = await crypto.subtle.sign("HMAC", key, utf8(`${id}.${timestamp}.${body}`));
    const expected = btoa(String.fromCharCode(...new Uint8Array(mac)));
    if (signatures.some((s) => sameString(s, expected))) return true;
  }
  return false;
}

interface PolarCustomer {
  email?: string | null;
  external_id?: string | null;
}
interface PolarSubscription {
  id: string;
  status: string;
  current_period_end?: string | null;
  cancel_at_period_end?: boolean;
  ends_at?: string | null;
  ended_at?: string | null;
  customer_id: string;
  customer?: PolarCustomer;
  product_id?: string;
  metadata?: Record<string, unknown>;
  modified_at?: string | null;
}

const userIdOf = (customer: PolarCustomer | undefined, metadata: Record<string, unknown> | undefined) =>
  customer?.external_id ?? (typeof metadata?.user_id === "string" ? metadata.user_id : null);

function eventsFor(env: Env, type: string, data: Record<string, unknown>, timestamp: string | undefined): BillingEvent[] {
  // Only the Sync plan's subscription events matter; orders (each renewal is one) change nothing here.
  if (!type.startsWith("subscription.")) return [];
  const sub = data as unknown as PolarSubscription;
  if (sub.product_id !== env.POLAR_PRODUCT_SYNC) return [];
  return [
    {
      subscriptionId: sub.id,
      customerId: sub.customer_id,
      email: sub.customer?.email?.toLowerCase() ?? null,
      userId: userIdOf(sub.customer, sub.metadata),
      status: sub.status,
      currentPeriodEnd: sub.current_period_end ?? null,
      cancelAt: sub.ended_at ?? (sub.cancel_at_period_end ? (sub.ends_at ?? sub.current_period_end ?? null) : null),
      at: sub.modified_at ?? timestamp ?? new Date().toISOString(),
    },
  ];
}

export const polar: BillingProvider = {
  async createCheckout(env, { user, successUrl, returnUrl }) {
    if (!env.POLAR_PRODUCT_SYNC) throw new ApiError(503, "not_configured", "Checkout isn't available yet.");
    const checkout = await polarApi<{ url: string }>(env, "POST", "/v1/checkouts/", {
      products: [env.POLAR_PRODUCT_SYNC],
      success_url: successUrl,
      return_url: returnUrl,
      customer_email: user.email,
      external_customer_id: user.id,
      metadata: { plan: "sync", user_id: user.id },
    });
    return checkout.url;
  },

  async portalUrl(env, { userId, customerId }, returnUrl) {
    const session = await polarApi<{ customer_portal_url: string }>(env, "POST", "/v1/customer-sessions/", {
      ...(customerId ? { customer_id: customerId } : { external_customer_id: userId }),
      return_url: returnUrl,
    });
    return session.customer_portal_url;
  },

  async cancelSubscription(env, subscriptionId) {
    await polarApi(env, "DELETE", `/v1/subscriptions/${encodeURIComponent(subscriptionId)}`);
  },

  async readWebhook(env, request, body) {
    if (!env.POLAR_WEBHOOK_SECRET || !(await verifyWebhook(env.POLAR_WEBHOOK_SECRET, request.headers, body))) return null;
    const payload = JSON.parse(body) as { type: string; timestamp?: string; data: Record<string, unknown> };
    return {
      id: request.headers.get("webhook-id")!,
      type: payload.type,
      events: eventsFor(env, payload.type, payload.data ?? {}, payload.timestamp),
    };
  },

  async metrics(env, { start, end, interval }, slugs) {
    if (!env.POLAR_ACCESS_TOKEN) throw new ApiError(503, "not_configured", "POLAR_ACCESS_TOKEN isn't set, so there are no sales numbers.");
    const query = new URLSearchParams({ start_date: start, end_date: end, interval, timezone: "UTC" });
    for (const slug of slugs) query.append("metrics", slug);
    const path = `/v1/metrics/?${query}`;
    const response = await polarFetch(env, "GET", path);
    if (response.status === 401 || response.status === 403)
      throw new ApiError(
        502,
        "missing_scope",
        "Polar refused: the access token needs the metrics:read scope. Make a token with it and run npm run polar:setup again.",
      );
    if (!response.ok) throw await unavailable("GET", path, response);
    const data = (await response.json()) as PolarMetrics;
    const pick = (row: Record<string, number | null | string>) =>
      Object.fromEntries(slugs.map((slug) => [slug, typeof row[slug] === "number" ? (row[slug] as number) : null]));
    return {
      metrics: slugs.flatMap((slug) => {
        const metric = data.metrics?.[slug];
        if (!metric) return [];
        // currency_sub_cent is still cents, with fractions.
        const type: Metrics["metrics"][number]["type"] = metric.type.startsWith("currency")
          ? "currency"
          : metric.type === "percentage"
            ? "percentage"
            : "scalar";
        return [{ slug, name: metric.display_name, type }];
      }),
      totals: pick(data.totals ?? {}),
      periods: (data.periods ?? []).map((period) => ({ start: period.timestamp, ...pick(period) })),
    };
  },
};

/** For tests and the local dev script: signs a body the way Polar does. */
export async function signWebhook(secret: string, id: string, timestamp: number, body: string): Promise<string> {
  const raw = fromBase64url(secret.replace(/^whsec_/, "").replace(/\+/g, "-").replace(/\//g, "_"));
  const key = await crypto.subtle.importKey("raw", raw, { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const mac = await crypto.subtle.sign("HMAC", key, utf8(`${id}.${timestamp}.${body}`));
  return `v1,${btoa(String.fromCharCode(...new Uint8Array(mac)))}`;
}
