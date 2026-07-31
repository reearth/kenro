// kenro browser demo: drop a GeoPackage, query it with spatial SQL, preview
// geometries — all client-side. SQLite = official SQLite WASM build;
// spatial functions = kenro-wasm registered as JS UDFs.

import sqlite3InitModule from "https://cdn.jsdelivr.net/npm/@sqlite.org/sqlite-wasm@3.50.0-build1/sqlite-wasm/jswasm/sqlite3.mjs";

import initKenro, * as kenroWasm from "./pkg/kenro_wasm.js";
import { registerKenro } from "./adapters/sqlite-wasm.mjs";

const $ = (id) => document.getElementById(id);
const statusEl = $("status");

function setStatus(message, isError = false) {
  statusEl.textContent = message;
  statusEl.classList.toggle("error", isError);
}

await initKenro();
const sqlite3 = await sqlite3InitModule();
let db = null;

// The only place a database comes into existence — keeps the door open for
// other openers (OPFS, remote readers) without touching the query UI.
function openFromBytes(bytes) {
  const next = new sqlite3.oo1.DB();
  if (bytes) {
    const p = sqlite3.wasm.allocFromTypedArray(bytes);
    const rc = sqlite3.capi.sqlite3_deserialize(
      next.pointer,
      "main",
      p,
      bytes.length,
      bytes.length,
      sqlite3.capi.SQLITE_DESERIALIZE_FREEONCLOSE,
    );
    next.checkRc(rc);
  }
  registerKenro(next, kenroWasm);
  db?.close();
  db = next;
  refreshLayers();
}

function sampleDatabase() {
  openFromBytes(null);
  db.exec(`
    CREATE TABLE parks (fid INTEGER PRIMARY KEY, name TEXT, geom BLOB);
    INSERT INTO parks (name, geom) VALUES
      ('yoyogi',   ST_AsGPB(ST_GeomFromText('POLYGON((139.694 35.669,139.700 35.669,139.700 35.675,139.694 35.675,139.694 35.669))', 4326))),
      ('ueno',     ST_AsGPB(ST_GeomFromText('POLYGON((139.770 35.712,139.776 35.712,139.776 35.718,139.770 35.718,139.770 35.712))', 4326))),
      ('fountain', ST_AsGPB(ST_GeomFromText('POINT(139.697 35.672)', 4326)));
  `);
  $("sql").value =
    "SELECT name,\n" +
    "       ST_AsText(ST_Centroid(geom)) AS centroid,\n" +
    "       round(ST_Area(ST_Transform(ST_GeomFromGPB(geom), 6677))) AS area_m2,\n" +
    "       h3_cell_to_string(h3_latlng_to_cell(ST_Centroid(geom), 9)) AS h3\n" +
    "FROM parks";
  setStatus("Sample database loaded — drop a .gpkg to replace it.");
  runSql();
}

function refreshLayers() {
  const container = $("layers");
  container.replaceChildren();
  let layers = [];
  try {
    db.exec({
      sql: "SELECT c.table_name, g.column_name FROM gpkg_contents c JOIN gpkg_geometry_columns g USING (table_name) WHERE c.data_type = 'features'",
      rowMode: "array",
      callback: (row) => layers.push(row),
    });
  } catch {
    try {
      layers = db
        .selectArrays(
          "SELECT name, 'geom' FROM sqlite_master WHERE type = 'table' AND sql LIKE '%geom BLOB%'",
        );
    } catch {
      layers = [];
    }
  }
  for (const [table, geomColumn] of layers) {
    const button = document.createElement("button");
    button.className = "secondary";
    button.textContent = table;
    button.onclick = () => {
      $("sql").value =
        `SELECT *, ST_AsText(ST_Centroid(${geomColumn})) AS centroid\nFROM "${table}" LIMIT 100`;
      runSql();
    };
    container.append(button);
  }
}

function renderTable(columns, rows) {
  const table = document.createElement("table");
  const head = table.createTHead().insertRow();
  for (const c of columns) {
    const th = document.createElement("th");
    th.textContent = c;
    head.append(th);
  }
  const body = table.createTBody();
  for (const row of rows) {
    const tr = body.insertRow();
    for (const value of row) {
      const td = tr.insertCell();
      if (value instanceof Uint8Array) {
        td.textContent = `GPB(${value.length} bytes)`;
        td.className = "blob";
        td.title = "click to decode with ST_AsText";
        td.onclick = () => {
          try {
            td.textContent = kenroWasm.stAsText(value);
          } catch (e) {
            td.textContent = String(e.message ?? e);
          }
          td.className = "";
        };
      } else {
        td.textContent = value === null ? "NULL" : String(value);
      }
    }
  }
  $("table").replaceChildren(table);
  $("rowcount").textContent = `(${rows.length} rows)`;
}

function runSql() {
  if (!db) return;
  const sql = $("sql").value.trim();
  if (!sql) return;
  const rows = [];
  let columns = [];
  try {
    const start = performance.now();
    db.exec({
      sql,
      rowMode: "array",
      columnNames: columns,
      callback: (row) => rows.push(row),
    });
    renderTable(columns, rows);
    setStatus(`OK — ${rows.length} rows in ${(performance.now() - start).toFixed(1)} ms`);
  } catch (e) {
    setStatus(String(e.message ?? e), true);
  }
}

// ---- SVG preview (hand-rolled GeoJSON renderer, equirectangular fit) ----

function collectCoords(geometry, into) {
  const walk = (c) => {
    if (typeof c[0] === "number") into.push(c);
    else c.forEach(walk);
  };
  if (geometry.type === "GeometryCollection") {
    geometry.geometries.forEach((g) => collectCoords(g, into));
  } else if (geometry.coordinates.length) {
    walk(geometry.coordinates);
  }
}

function previewGeometries() {
  if (!db) return;
  const sql = $("sql").value.trim();
  const geoms = [];
  try {
    db.exec({
      sql,
      rowMode: "array",
      callback: (row) => {
        for (const value of row) {
          if (!(value instanceof Uint8Array)) continue;
          try {
            let blob = value;
            const srid = kenroWasm.stSrid(blob);
            if (srid !== 0 && srid !== 4326) {
              blob = kenroWasm.stTransform(blob, 4326);
            }
            geoms.push(JSON.parse(kenroWasm.stAsGeojson(blob)));
          } catch {
            /* not a geometry blob — skip */
          }
        }
      },
    });
  } catch (e) {
    setStatus(String(e.message ?? e), true);
    return;
  }
  if (!geoms.length) {
    setStatus("no geometry BLOB column in the result — nothing to preview", true);
    return;
  }
  const all = [];
  geoms.forEach((g) => collectCoords(g, all));
  const xs = all.map((c) => c[0]);
  const ys = all.map((c) => c[1]);
  const [minX, maxX] = [Math.min(...xs), Math.max(...xs)];
  const [minY, maxY] = [Math.min(...ys), Math.max(...ys)];
  const span = Math.max(maxX - minX, maxY - minY) || 1;
  const sx = (x) => 2 + ((x - minX) / span) * 96;
  const sy = (y) => 98 - ((y - minY) / span) * 96;

  const svg = $("map");
  svg.replaceChildren();
  const ns = "http://www.w3.org/2000/svg";
  const ring = (coords) =>
    coords.map((c, i) => `${i ? "L" : "M"}${sx(c[0]).toFixed(2)} ${sy(c[1]).toFixed(2)}`).join("") + "Z";
  const emit = (geometry) => {
    const { type, coordinates } = geometry;
    if (type === "GeometryCollection") return geometry.geometries.forEach(emit);
    if (!coordinates?.length) return;
    if (type === "Point" || type === "MultiPoint") {
      for (const c of type === "Point" ? [coordinates] : coordinates) {
        const dot = document.createElementNS(ns, "circle");
        dot.setAttribute("cx", sx(c[0]).toFixed(2));
        dot.setAttribute("cy", sy(c[1]).toFixed(2));
        dot.setAttribute("r", "1.4");
        svg.append(dot);
      }
      return;
    }
    const rings =
      type === "LineString" ? [coordinates]
      : type === "Polygon" || type === "MultiLineString" ? coordinates
      : coordinates.flat(); // MultiPolygon
    const path = document.createElementNS(ns, "path");
    path.setAttribute("d", rings.map(ring).join(""));
    if (type.includes("LineString")) path.setAttribute("fill", "none");
    svg.append(path);
  };
  geoms.forEach(emit);
  setStatus(`previewing ${geoms.length} geometries (WGS84, equirectangular fit)`);
}

// ---- wiring ----

const drop = $("drop");
drop.onclick = () => {
  const input = document.createElement("input");
  input.type = "file";
  input.accept = ".gpkg,.sqlite,.db";
  input.onchange = () => input.files[0] && loadFile(input.files[0]);
  input.click();
};
drop.ondragover = (e) => {
  e.preventDefault();
  drop.classList.add("armed");
};
drop.ondragleave = () => drop.classList.remove("armed");
drop.ondrop = (e) => {
  e.preventDefault();
  drop.classList.remove("armed");
  e.dataTransfer.files[0] && loadFile(e.dataTransfer.files[0]);
};

async function loadFile(file) {
  try {
    openFromBytes(new Uint8Array(await file.arrayBuffer()));
    setStatus(`${file.name} loaded (${(file.size / 1024).toFixed(0)} KB) — pick a layer or write SQL.`);
    $("table").replaceChildren();
    $("rowcount").textContent = "";
  } catch (e) {
    setStatus(String(e.message ?? e), true);
  }
}

$("run").onclick = runSql;
$("preview").onclick = previewGeometries;
$("sql").addEventListener("keydown", (e) => {
  if ((e.metaKey || e.ctrlKey) && e.key === "Enter") runSql();
});

sampleDatabase();
