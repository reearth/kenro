// HTTP front for both backends — the same routes, the same request bodies,
// only the storage behind them differs:
//
//   POST /load     GeoJSON FeatureCollection body
//   POST /query    {"wkt": "...", "predicate": "...", "srid": 3857, ...}
//   GET  /stats
//   POST /clear
//
//   ?backend=do (default) | d1
//   ?shard=<name>   picks the Durable Object; ignored by D1
//
// One DO per region/tile/tenant is the natural way to scale the DO path:
// each holds its own SQLite and its own copy of the wasm, and they run in
// parallel with no coordination.

import { D1SpatialIndex } from "./spatial-d1.mjs";

export { SpatialIndex } from "./spatial-do.mjs";

const json = (body, status = 200) =>
  new Response(JSON.stringify(body, null, 2), {
    status,
    headers: { "content-type": "application/json" },
  });

function store(env, url) {
  const backend = url.searchParams.get("backend") ?? "do";
  switch (backend) {
    case "do": {
      const shard = url.searchParams.get("shard") ?? "default";
      return env.SPATIAL.get(env.SPATIAL.idFromName(shard));
    }
    case "d1":
      return new D1SpatialIndex(env.DB);
    default:
      throw new Error(`unknown backend: ${backend}`);
  }
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);

    try {
      const target = store(env, url);
      switch (`${request.method} ${url.pathname}`) {
        case "POST /load":
          return json(await target.load(await request.json()));
        case "POST /query":
          return json(await target.query(await request.json()));
        case "GET /stats":
          return json(await target.stats());
        case "POST /clear":
          return json(await target.clear());
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
