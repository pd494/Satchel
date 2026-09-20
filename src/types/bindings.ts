import type { Account } from "../db";

export interface WorkerBindings {
  readonly ACCOUNTS: DurableObjectNamespace<Account>;
  readonly WEBHOOK_SECRET?: string;
  readonly ACCOUNT_ID_SECRET?: string;
}
