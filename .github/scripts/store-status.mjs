// Reads one Microsoft Store submission; it never changes Store state.
// Schema/auth: https://learn.microsoft.com/windows/uwp/monetize/create-and-manage-submissions-using-windows-store-services
// https://learn.microsoft.com/windows/uwp/monetize/get-an-app-submission
// https://learn.microsoft.com/windows/uwp/monetize/get-status-for-an-app-submission
import { appendFile, mkdir, writeFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';

export const APP_ID = '9PMQMBZJG06R';
const API = 'https://manage.devcenter.microsoft.com';
const statuses = new Set(['None','Canceled','PendingCommit','CommitStarted','CommitFailed','PendingPublication','Publishing','Published','PublishFailed','PreProcessing','PreProcessingFailed','Certification','CertificationFailed','Release','ReleaseFailed']);
const failureCodes = new Set(['None','InvalidArchive','MissingFiles','PackageValidationFailed','InvalidParameterValue','InvalidOperation','InvalidState','ResourceNotFound','ServiceError','ListingOptOutWarning','ListingOptInWarning','UpdateOnlyWarning','Other','PackageValidationWarning']);
const submissionId = value => typeof value === 'string' && /^[1-9]\d{0,31}$/.test(value) ? value : null;
const packageVersion = value => typeof value === 'string' && /^\d{1,5}\.\d{1,5}\.\d{1,5}\.\d{1,5}$/.test(value) && value.split('.').every(n=>Number(n)<=65535) ? value : null;
const statusName = value => statuses.has(value) ? value : 'Unknown';

export function validateInputs(id, version) {
  if (!submissionId(id)) throw new Error('submission_id must be a canonical decimal string.');
  if (!packageVersion(version)) throw new Error('expected_version must be a four-part MSIX version.');
}

export function summarize(app, submission, status, id, expectedVersion) {
  validateInputs(id, expectedVersion);
  if (app?.id !== APP_ID) throw new Error('The Store response does not match the requested application.');
  if (submission?.id !== id) throw new Error('The Store response does not match the requested submission.');
  if (!statuses.has(status?.status)) throw new Error('The Store response has an unknown submission status.');
  if (!Array.isArray(submission.applicationPackages)) throw new Error('The Store response has no package list.');
  const packages = submission.applicationPackages.map(p => ({
    version: packageVersion(p.version),
    architecture: ['ARM','ARM64','X86','X64','Neutral'].includes(p.architecture) ? p.architecture : 'Other',
    fileStatus: ['None','PendingUpload','Uploaded','PendingDelete'].includes(p.fileStatus) ? p.fileStatus : 'Unknown',
  }));
  const lastPublished = submissionId(app.lastPublishedApplicationSubmission?.id);
  const pending = submissionId(app.pendingApplicationSubmission?.id);
  const expectedVersionPresent = packages.some(p => p.version === expectedVersion && p.fileStatus === 'Uploaded');
  const publishedExpectedVersion = status.status === 'Published' && lastPublished === id && expectedVersionPresent;
  const rollout = submission.packageDeliveryOptions?.packageRollout;
  const percentage = typeof rollout?.packageRolloutPercentage === 'number' && rollout.packageRolloutPercentage >= 0 && rollout.packageRolloutPercentage <= 100 ? rollout.packageRolloutPercentage : null;
  const fullyRolledOut = rollout?.isPackageRollout !== true || percentage === 100;
  const details = status.statusDetails ?? {};
  const codes = rows => Array.isArray(rows) ? rows.map(r => failureCodes.has(r.code) ? r.code : 'Other') : [];
  return {
    checkedAt: new Date().toISOString(), applicationId: APP_ID, submissionId: id, expectedVersion,
    status: status.status, submissionStatus: statusName(submission.status),
    lastPublishedApplicationSubmission: lastPublished, pendingApplicationSubmission: pending,
    packages, expectedVersionPresent, publishedExpectedVersion, fullyRolledOut,
    available: publishedExpectedVersion && fullyRolledOut,
    rollout: { enabled: rollout?.isPackageRollout === true, percentage },
    errors: codes(details.errors), warnings: codes(details.warnings),
    certificationReportCount: Array.isArray(details.certificationReports) ? details.certificationReports.length : 0,
  };
}

// Never include response bodies or request objects in errors: they may contain access
// tokens, upload SAS URLs, certification report links, or private listing metadata.
async function readJson(fetcher, url, options, label) {
  let response;
  try { response = await fetcher(url, { ...options, redirect: 'error', signal: AbortSignal.timeout(20_000) }); }
  catch { throw new Error(`${label} request failed or timed out.`); }
  if (!response.ok) throw new Error(`${label} returned HTTP ${response.status}.`);
  try { return await response.json(); }
  catch { throw new Error(`${label} returned invalid JSON.`); }
}

export async function collectStatus(env, fetcher = fetch) {
  const id = env.SUBMISSION_ID, version = env.EXPECTED_VERSION ?? '1.0.11.0';
  validateInputs(id, version);
  const tenant = env.MSSTORE_TENANT_ID;
  if (!tenant || !/^[A-Za-z0-9.-]+$/.test(tenant)) throw new Error('A valid MSSTORE_TENANT_ID is required.');
  if (!env.MSSTORE_CLIENT_ID || !env.MSSTORE_CLIENT_SECRET) throw new Error('Microsoft Store client credentials are required.');
  const token = await readJson(fetcher, `https://login.microsoftonline.com/${encodeURIComponent(tenant)}/oauth2/token`, {
    method: 'POST', headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams({ grant_type: 'client_credentials', client_id: env.MSSTORE_CLIENT_ID, client_secret: env.MSSTORE_CLIENT_SECRET, resource: API }),
  }, 'Microsoft identity token');
  if (typeof token.access_token !== 'string' || !token.access_token) throw new Error('Microsoft identity returned no access token.');
  const headers = { Authorization: `Bearer ${token.access_token}`, Accept: 'application/json' };
  const base = `${API}/v1.0/my/applications/${APP_ID}`;
  const reads = await Promise.allSettled([
    readJson(fetcher, base, { method: 'GET', headers }, 'Store application'),
    readJson(fetcher, `${base}/submissions/${id}`, { method: 'GET', headers }, 'Store submission'),
    readJson(fetcher, `${base}/submissions/${id}/status`, { method: 'GET', headers }, 'Store submission status'),
  ]);
  const failure = reads.find(r => r.status === 'rejected');
  if (failure) throw failure.reason;
  return summarize(...reads.map(r => r.value), id, version);
}

export function summaryMarkdown(result) {
  return `## Microsoft Store submission status\n\n` +
    `Application: ${result.applicationId}\n\nSubmission: ${result.submissionId}\n\n` +
    `Status: **${result.status}**\n\nExpected package: **${result.expectedVersion}** (${result.expectedVersionPresent ? 'present' : 'not confirmed'})\n\n` +
    `Last published submission: ${result.lastPublishedApplicationSubmission ?? 'none'}\n\nPending submission: ${result.pendingApplicationSubmission ?? 'none'}\n\n` +
    `Published with this version: **${result.publishedExpectedVersion ? 'yes' : 'no'}**\n\nFully rolled out: **${result.fullyRolledOut ? 'yes' : 'no'}**\n\n` +
    `Availability confirmed: **${result.available ? 'yes' : 'no'}**\n\n` +
    `Package versions: ${result.packages.map(p=>`${p.version ?? 'unknown'} (${p.architecture}, ${p.fileStatus})`).join(', ') || 'none'}\n\n` +
    `Errors: ${result.errors.join(', ') || 'none'}; warnings: ${result.warnings.join(', ') || 'none'}; certification reports: ${result.certificationReportCount}.\n\n` +
    `This is a read-only snapshot, not a submission or publishing action. Pending certification is not availability.\n`;
}

async function main() {
  await mkdir('store-status', { recursive: true });
  try {
    const result = await collectStatus(process.env);
    const summary = summaryMarkdown(result);
    await writeFile('store-status/status.json', JSON.stringify(result, null, 2) + '\n');
    await writeFile('store-status/summary.md', summary);
    if (process.env.GITHUB_STEP_SUMMARY) await appendFile(process.env.GITHUB_STEP_SUMMARY, summary);
    if (process.env.GITHUB_OUTPUT) await appendFile(process.env.GITHUB_OUTPUT, `available=${result.available}\nstatus=${result.status}\n`);
    console.log(summary);
    if (result.status.endsWith('Failed') || result.status === 'Canceled') process.exitCode = 1;
  } catch (error) {
    // collectStatus only throws locally constructed, non-sensitive messages.
    const message = error instanceof Error ? error.message : 'Store status check failed.';
    await writeFile('store-status/error.json', JSON.stringify({ checkedAt: new Date().toISOString(), error: message }, null, 2) + '\n');
    if (process.env.GITHUB_STEP_SUMMARY) await appendFile(process.env.GITHUB_STEP_SUMMARY, `## Microsoft Store status check failed\n\n${message}\n`);
    console.error(message);
    process.exitCode = 1;
  }
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) await main();
