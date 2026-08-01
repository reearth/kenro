// The two backends must agree on the schema: the Durable Object creates its
// tables in code, D1 gets them from migrations/. Nothing enforces that at
// runtime, so it is enforced here.
import { expect, it } from "vitest";

import migrationSql from "../migrations/0001_init.sql?raw";
import { SCHEMA } from "../src/spatial.mjs";

/** CREATE TABLE statements, normalized: no comments, single-spaced, no `;`. */
function tables(sql) {
  return sql
    .replace(/--[^\n]*/g, "")
    .split(";")
    .map((s) => s.replace(/\s+/g, " ").trim())
    .filter((s) => s.startsWith("CREATE TABLE"))
    .sort();
}

it("the DO schema and the D1 migration define the same tables", () => {
  expect(tables(migrationSql)).toEqual(tables(SCHEMA.join(";")));
});
