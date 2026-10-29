import assert from 'node:assert/strict';
import test from 'node:test';
import { APP_ID, collectStatus, summarize, summaryMarkdown, validateInputs } from './store-status.mjs';

const id = '1152921505702247644';
const version = '1.0.11.0';
const app = { id: APP_ID, lastPublishedApplicationSubmission: { id }, pendingApplicationSubmission: null };
const submission = { id, status: 'Published', applicationPackages: [{ version, architecture: 'X64', fileStatus: 'Uploaded' }] };
const status = { status: 'Published', statusDetails: { errors: [], warnings: [], certificationReports: [] } };
const env = { SUBMISSION_ID: id, EXPECTED_VERSION: version, MSSTORE_TENANT_ID: 'synthetic-tenant', MSSTORE_CLIENT_ID: 'synthetic-client', MSSTORE_CLIENT_SECRET: 'synthetic-client-secret' };

test('availability requires published status, exact last-published submission, and an uploaded expected package', () => {
  assert.equal(summarize(app, submission, status, id, version).available, true);
  for (const [a, s, st] of [
    [app, submission, { status: 'Certification' }],
    [{ ...app, lastPublishedApplicationSubmission: { id: '123' } }, submission, status],
    [app, { ...submission, applicationPackages: [{ version: '1.0.10.0', fileStatus: 'Uploaded' }] }, status],
    [app, { ...submission, applicationPackages: [{ version, fileStatus: 'PendingDelete' }] }, status],
    [app, { ...submission, packageDeliveryOptions: { packageRollout: { isPackageRollout: true, packageRolloutPercentage: 25 } } }, status],
  ]) assert.equal(summarize(a, s, st, id, version).available, false);
});

test('wrong app/submission and imprecise or malformed identifiers are rejected', () => {
  assert.throws(() => summarize({ ...app, id: 'OTHERAPP' }, submission, status, id, version), /application/);
  assert.throws(() => summarize(app, { ...submission, id: '123' }, status, id, version), /submission/);
  for (const bad of [Number(id), '0123', '../status', '1\n', '']) assert.throws(() => validateInputs(bad, version));
  for (const bad of ['1.0.11', '65536.0.0.0', '1.0.11.0\n']) assert.throws(() => validateInputs(id, bad));
});

test('private listing fields, SAS URLs and diagnostic details never enter evidence', () => {
  const secret = 'DO-NOT-EMIT-THIS-SECRET';
  const result = summarize({ ...app, publisherName: secret }, {
    ...submission, fileUploadUrl: `https://storage.example/upload?sig=${secret}`, listings: { secret },
    applicationPackages: [{ ...submission.applicationPackages[0], fileName: secret }],
  }, { ...status, statusDetails: { errors: [{ code: 'PackageValidationFailed', details: secret }], warnings: [{ code: secret, details: secret }], certificationReports: [{ reportUrl: `https://example.test?sig=${secret}` }] } }, id, version);
  const output = JSON.stringify(result) + summaryMarkdown(result);
  assert(!output.includes(secret));
  assert.deepEqual(result.errors, ['PackageValidationFailed']);
  assert.deepEqual(result.warnings, ['Other']);
  assert.equal(result.certificationReportCount, 1);
});

test('token POST is followed only by the three exact authenticated read endpoints', async () => {
  const calls = [];
  const fixture = async (url, options) => {
    calls.push({ url, options });
    assert.equal(options.redirect, 'error');
    assert(options.signal instanceof AbortSignal);
    if (url.startsWith('https://login.microsoftonline.com/')) {
      assert.equal(options.method, 'POST');
      assert.equal(options.body.get('resource'), 'https://manage.devcenter.microsoft.com');
      assert.equal(options.body.get('client_secret'), env.MSSTORE_CLIENT_SECRET);
      return { ok: true, json: async () => ({ access_token: 'synthetic-access-token' }) };
    }
    assert.equal(options.method, 'GET');
    assert.equal(options.headers.Authorization, 'Bearer synthetic-access-token');
    assert.equal(options.body, undefined);
    const data = url.endsWith('/status') ? status : url.endsWith(`/submissions/${id}`) ? submission : app;
    return { ok: true, json: async () => data };
  };
  const result = await collectStatus(env, fixture);
  assert.equal(result.available, true);
  assert.equal(calls.length, 4);
  const base = `https://manage.devcenter.microsoft.com/v1.0/my/applications/${APP_ID}`;
  assert.deepEqual(calls.slice(1).map(c => c.url).sort(), [base, `${base}/submissions/${id}`, `${base}/submissions/${id}/status`].sort());
  assert(!JSON.stringify(result).includes('synthetic-access-token'));
  assert(!JSON.stringify(result).includes(env.MSSTORE_CLIENT_SECRET));
});

test('provider errors do not expose token, response body or exception details', async () => {
  const sensitive = 'secret-token-and-request-body';
  await assert.rejects(collectStatus(env, async () => { throw Error(sensitive); }), error => !error.message.includes(sensitive) && /failed or timed out/.test(error.message));
  await assert.rejects(collectStatus(env, async () => ({ ok: false, status: 401, json: async () => ({ error_description: sensitive }) })), /token returned HTTP 401\./);
  await assert.rejects(collectStatus(env, async (url) => url.includes('login.microsoftonline.com')
    ? { ok: true, json: async () => ({ access_token: sensitive }) }
    : { ok: false, status: 403, json: async () => ({ sensitive }) }), error => !error.message.includes(sensitive) && /returned HTTP 403/.test(error.message));
});

test('invalid inputs fail before credentials or network are used', async () => {
  let called = false;
  await assert.rejects(collectStatus({ ...env, SUBMISSION_ID: '../another-app' }, () => { called = true; }), /canonical decimal/);
  await assert.rejects(collectStatus({ ...env, MSSTORE_TENANT_ID: '../another-host' }, () => { called = true; }), /TENANT_ID/);
  assert.equal(called, false);
});
