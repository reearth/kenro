// HTTP front for the spatial Durable Object.
//
//   POST /load?shard=…    GeoJSON FeatureCollection body
//   POST /query?shard=…   {"wkt": "...", "predicate": "...", ...}
//   GET  /stats?shard=…
//   POST /clear?shard=…
//
// `shard` picks the Durable Object: one DO per region/tile/tenant is the
// natural way to scale this — each holds its own SQLite and its own copy of
// the wasm, and they run in parallel with no coordination.

export { SpatialIndex } from "./spatial-do.mjs";

const json = (body, status = 200) =>
  new Response(JSON.stringify(body, null, 2), {
    status,
    headers: { "content-type": "application/json" },
  });

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const shard = url.searchParams.get("shard") ?? "default";
    const stub = env.SPATIAL.get(env.SPATIAL.idFromName(shard));

    try {
      switch (`${request.method} ${url.pathname}`) {
        case "POST /load":
          return json(await stub.load(await request.json()));
        case "POST /query":
          return json(await stub.query(await request.json()));
        case "GET /stats":
          return json(await stub.stats());
        case "POST /clear":
          return json(await stub.clear());
        default:
          return json({ error: "not found", routes: ["/load", "/query", "/stats", "/clear"] }, 404);
      }
    } catch (e) {
      // kenro's errors are already prefixed `kenro: …` — pass them through
      // verbatim so the SQL-side wording matches every other binding.
      return json({ error: String(e.message ?? e) }, 400);
    }
  },
};
