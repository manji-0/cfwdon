// Query-count entry for local measurement only (scripts/query_count/run.sh
// copies it next to the built worker). It counts every D1 statement the
// Worker runs, the serial rounds they form (a round starts when no other
// statement is in flight), and the tally when the response is returned;
// GET /__qc returns and resets it.
import Base from "./index.js";
export { ContainerStartupOptions, MinifyConfig, R2Range, RateLimitCounter, StreamHub } from "./index.js";

const tally = { statements: 0, rounds: 0, batches: 0, sql: [] };
let inFlight = 0;
const track = async (n, sql, work) => {
  if (inFlight === 0) tally.rounds += 1;
  inFlight += 1;
  tally.statements += n;
  if (sql) tally.sql.push(...sql);
  try { return await work(); } finally { inFlight -= 1; }
};
const SQL = Symbol("sql");
let patched = false;
let atResponse;
const patch = (db) => {
  if (patched) return;
  patched = true;
  const dbProto = Object.getPrototypeOf(db);
  const prepare = dbProto.prepare;
  dbProto.prepare = function (sql) { const s = prepare.call(this, sql); try { s[SQL] = sql; } catch {} return s; };
  const stmtProto = Object.getPrototypeOf(prepare.call(db, "SELECT 1"));
  const bind = stmtProto.bind;
  stmtProto.bind = function (...a) { const s = bind.apply(this, a); try { s[SQL] = this[SQL]; } catch {} return s; };
  for (const m of ["first", "run", "all", "raw"]) {
    const o = stmtProto[m];
    if (typeof o === "function") stmtProto[m] = function (...a) { return track(1, [this.statement ?? this[SQL]], () => o.apply(this, a)); };
  }
  const batch = dbProto.batch;
  dbProto.batch = function (st) { tally.batches += 1; return track(st.length, st.map((s) => s.statement ?? s[SQL]), () => batch.call(this, st)); };
};

export default class extends Base {
  async fetch(request) {
    patch(this.env.DB);
    const url = new URL(request.url);
    if (url.pathname === "/__qc") {
      const out = { ...tally, atResponse };
      tally.statements = 0; tally.rounds = 0; tally.batches = 0; tally.sql = [];
      atResponse = undefined;
      return Response.json(out);
    }
    const response = await super.fetch(request);
    atResponse = { ...tally, sql: [...tally.sql] };
    return response;
  }
}
