// Runs the Worker + Durable Object in workerd (the real runtime, real
// SQLite, real wasm) via @cloudflare/vitest-pool-workers.
import { SELF } from "cloudflare:test";
import { beforeEach, expect, it } from "vitest";

const post = async (path, body) => {
  const res = await SELF.fetch(`http://x${path}`, {
    method: "POST",
    body: JSON.stringify(body),
  });
  return { status: res.status, body: await res.json() };
};

function square(id, cx, cy, half = 0.05) {
  return {
    type: "Feature",
    id,
    properties: { name: id },
    geometry: {
      type: "Polygon",
      coordinates: [
        [
          [cx - half, cy - half],
          [cx + half, cy - half],
          [cx + half, cy + half],
          [cx - half, cy + half],
          [cx - half, cy - half],
        ],
      ],
    },
  };
}

const FIXTURE = {
  type: "FeatureCollection",
  features: [
    square("tokyo", 139.7, 35.68),
    square("yokohama", 139.63, 35.44),
    square("osaka", 135.5, 34.69),
    square("sapporo", 141.35, 43.06),
    // Spans far more tiles than MAX_CELLS at z8 → filed as OVERSIZED.
    {
      type: "Feature",
      id: "wide",
      properties: { name: "wide" },
      geometry: {
        type: "Polygon",
        coordinates: [
          [
            [120, 20],
            [150, 20],
            [150, 46],
            [120, 46],
            [120, 20],
          ],
        ],
      },
    },
  ],
};

beforeEach(async () => {
  await post("/clear", {});
  const { body } = await post("/load", FIXTURE);
  expect(body.inserted).toBe(5);
});

it("indexes tile cells, with the huge feature marked oversized", async () => {
  const res = await SELF.fetch("http://x/stats");
  const stats = await res.json();
  expect(stats.features).toBe(5);
  expect(stats.oversized).toBe(1);
});

it("intersects: returns only the features the window really hits", async () => {
  const { body } = await post("/query", {
    wkt: "POLYGON((139.6 35.6, 139.8 35.6, 139.8 35.75, 139.6 35.75, 139.6 35.6))",
  });
  const ids = body.features.map((f) => f.id).sort();
  // `wide` covers all of Japan, so it is a true hit — not an index artifact.
  expect(ids).toEqual(["tokyo", "wide"]);
  // The coarse filter did its job: not every row reached the predicate.
  expect(body.stats.refined).toBeLessThan(FIXTURE.features.length);
});

it("within: the window must contain the feature", async () => {
  const { body } = await post("/query", {
    wkt: "POLYGON((139.5 35.5, 140 35.5, 140 36, 139.5 36, 139.5 35.5))",
    predicate: "within",
  });
  expect(body.features.map((f) => f.id)).toEqual(["tokyo"]);
});

it("dwithin: pads the coarse filter so near-misses are not dropped", async () => {
  const near = { wkt: "POINT(139.7 35.9)", predicate: "dwithin", distance: 0.2 };
  const { body } = await post("/query", near);
  expect(body.features.map((f) => f.id).sort()).toEqual(["tokyo", "wide"]);

  // Without the padding this same query would find nothing: the point's own
  // bbox touches no feature bbox at all.
  const { body: tight } = await post("/query", { ...near, distance: 0.01 });
  expect(tight.features.map((f) => f.id)).toEqual(["wide"]);
});

it("srid: output geometry is reprojected by kenro", async () => {
  const { body } = await post("/query", {
    wkt: "POLYGON((139.6 35.6, 139.8 35.6, 139.8 35.75, 139.6 35.75, 139.6 35.6))",
    srid: 3857,
    limit: 1,
  });
  const [x, y] = body.features[0].geometry.coordinates[0][0];
  expect(x).toBeGreaterThan(1e7); // Web Mercator metres, not degrees
  expect(y).toBeGreaterThan(4e6);
});

it("reports kenro's own error wording", async () => {
  const { status, body } = await post("/query", { wkt: "NOT WKT" });
  expect(status).toBe(400);
  expect(body.error).toMatch(/^kenro: /);
});

it("rejects an unknown predicate", async () => {
  const { status, body } = await post("/query", { wkt: "POINT(0 0)", predicate: "nope" });
  expect(status).toBe(400);
  expect(body.error).toMatch(/unknown predicate/);
});
