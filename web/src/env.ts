export interface Env {
  ASSETS: Fetcher;
  DB: D1Database;
  SYNC: R2Bucket;
  RELEASES: R2Bucket;

  APP_URL: string;
  EMAIL_FROM: string;
  SUPPORT_EMAIL: string;
  /** "sandbox" while testing, "production" once the payment provider has approved the account. */
  BILLING_ENV: "sandbox" | "production";
  /** The Polar product id of the monthly Sync plan, the only thing UrBudget sells. */
  POLAR_PRODUCT_SYNC: string;

  POLAR_ACCESS_TOKEN?: string;
  POLAR_WEBHOOK_SECRET?: string;
  RESEND_API_KEY?: string;
  /** Server-only Google service-account JSON, authorized for app.urbudget in Play Console. */
  GOOGLE_PLAY_SERVICE_ACCOUNT?: string;
  /** Authenticated Pub/Sub push identity and exact audience for Play notifications. */
  GOOGLE_PLAY_PUSH_EMAIL?: string;
  GOOGLE_PLAY_PUSH_AUDIENCE?: string;
  /** Optional dedicated synthetic-data review account; never a normal customer's email. */
  PLAY_REVIEW_EMAIL?: string;
  /** SHA-256 of `review:<lowercase email>:<random access code>`; keep as a Worker secret. */
  PLAY_REVIEW_CODE_HASH?: string;
  /** "1" in local development: sign-in responses include the code, and emails are only logged. */
  DEV_LOGIN_CODES?: string;
  /** Enables first-time sync v2 setup/migration only. Absent in production until approved. */
  SYNC_V2_ENABLED?: string;
  /** Emergency compatible rollback: retain v2 reads/receipts/deletion while pausing publication. */
  SYNC_V2_PUBLICATION_PAUSED?: string;

  /** The admin page sits behind Cloudflare Access: the team's domain (`<team>.cloudflareaccess.com`). */
  ACCESS_TEAM_DOMAIN?: string;
  /** The Access application's audience (AUD) tag. */
  ACCESS_AUD?: string;
  /**
   * Who may use the admin page, comma-separated; Access must let them through too. A Worker secret
   * (`npx wrangler secret put ADMIN_EMAILS`), so the addresses stay out of the repository.
   */
  ADMIN_EMAILS?: string;
  /** `owner/name` of the GitHub repository whose releases' download counts the admin page shows. */
  GITHUB_REPO?: string;
  /** Optional read-only GitHub token; unauthenticated requests share a low per-address rate limit. */
  GITHUB_TOKEN?: string;
  /**
   * Optional: the "UrBudget releases" Microsoft Entra application (Manager role in Partner Center),
   * the same one .github/workflows/store.yml signs in with, for the Store's acquisitions and installs.
   */
  MSSTORE_TENANT_ID?: string;
  MSSTORE_CLIENT_ID?: string;
  MSSTORE_CLIENT_SECRET?: string;
}

export interface User {
  id: string;
  email: string;
  billing_customer_id: string | null;
  created_at: string;
}

export interface Session {
  id: string;
  user_id: string;
  kind: "web" | "desktop" | "link";
  device_name: string | null;
  last_seen_at: string;
  expires_at: string;
}

export type AppEnv = {
  Bindings: Env;
  Variables: { user: User; session: Session; viaCookie: boolean; admin: string };
};
