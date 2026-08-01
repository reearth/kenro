// D1's schema comes from migrations/, so the test D1 needs them applied
// before any test runs. Setup-file state becomes the baseline that each
// test's isolated storage stacks on, so this happens once, not per test.
import { applyD1Migrations, env } from "cloudflare:test";

await applyD1Migrations(env.DB, env.TEST_MIGRATIONS);
