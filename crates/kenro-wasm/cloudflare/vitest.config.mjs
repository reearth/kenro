import { defineWorkersConfig, readD1Migrations } from "@cloudflare/vitest-pool-workers/config";

const migrations = await readD1Migrations("./migrations");

export default defineWorkersConfig({
  test: {
    setupFiles: ["./test/apply-migrations.mjs"],
    poolOptions: {
      workers: {
        wrangler: { configPath: "./wrangler.jsonc" },
        miniflare: {
          // Read by test/apply-migrations.mjs; not a binding the Worker uses.
          bindings: { TEST_MIGRATIONS: migrations },
        },
      },
    },
  },
});
