const table = "marks";
const reviewTable = "review";
let open;

function database() {
  if (open) return open;
  const request = indexedDB.open("repo-city-progress", 2);
  open = new Promise((resolve, reject) => {
    const wait = setTimeout(() => reject(new Error(
      "The progress database is taking too long to open. If another Repo City tab is open, reload it, then reload this page.",
    )), 3000);
    request.onupgradeneeded = event => {
      if (event.oldVersion === 0) request.result.createObjectStore(table);
      request.result.createObjectStore(reviewTable);
    };
    // Keep one pending upgrade. Starting another open here would queue behind it.
    request.onblocked = () => {
      clearTimeout(wait);
      reject(new Error("Another Repo City tab is blocking the progress update. Reload the other Repo City tabs, then reload this page to restore saved progress."));
    };
    request.onerror = () => {
      clearTimeout(wait);
      open = undefined;
      reject(request.error);
    };
    request.onsuccess = () => {
      clearTimeout(wait);
      const db = request.result;
      const connection = Promise.resolve(db);
      open = connection;
      const forget = () => { if (open === connection) open = undefined; };
      db.onversionchange = () => { db.close(); forget(); };
      db.onclose = forget;
      resolve(db);
    };
  });
  return open;
}

function scopedKeys(scope) {
  return IDBKeyRange.bound([scope], [scope, []]);
}

export async function readProgress(scope) {
  const db = await database();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(table);
    const request = tx.objectStore(table).getAll(scopedKeys(scope));
    tx.oncomplete = () => resolve(request.result);
    tx.onabort = () => reject(tx.error);
  });
}

// A null batch clears this comparison. Other batches update individual marks.
export async function writeProgress(scope, changes) {
  const db = await database();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(table, "readwrite");
    tx.oncomplete = () => resolve();
    tx.onabort = () => reject(tx.error);
    const store = tx.objectStore(table);
    if (changes === null) store.delete(scopedKeys(scope));
    else for (const [mark, checked] of changes) {
      if (checked) store.put(mark, [scope, mark]);
      else store.delete([scope, mark]);
    }
  });
}

export async function readReview(scope) {
  const db = await database();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(reviewTable);
    const read = tx.objectStore(reviewTable).getAll(scopedKeys(scope));
    tx.oncomplete = () => resolve(read.result);
    tx.onabort = () => reject(tx.error);
  });
}

async function updateReview(scope, id, value) {
  const db = await database();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(reviewTable, "readwrite");
    const store = tx.objectStore(reviewTable);
    tx.oncomplete = () => resolve();
    tx.onabort = () => reject(tx.error);
    if (value === null) store.delete([scope, id]);
    else store.put(value, [scope, id]);
  });
}
export const saveReview = (scope, id, value) => updateReview(scope, id, value);
export const deleteReview = (scope, id) => updateReview(scope, id, null);

export function carryVisits(marks, previous, current) {
  const visits = [];
  for (const mark of marks) {
    let saved;
    try { saved = JSON.parse(mark); } catch { continue; }
    if (!saved || typeof saved !== "object") continue;
    const interval = saved.reveal ?? saved.read_document;
    if (interval && saved.source && typeof saved.source.blob === "string"
      && Array.isArray(saved.source.lines) && saved.source.lines.every(line => typeof line === "string")) {
      const side = interval.before ? "before" : "after";
      if (current[side][interval.path]) visits.push([mark, true]);
      // Rust checks exact blob identity or maps unique unchanged source lines.
      continue;
    }
    const { visited } = saved;
    if (!visited) continue;
    const { path, diff } = visited;
    const before = current.before[path]?.id;
    const after = current.after[path]?.id;
    const oldBefore = previous.before[path]?.id;
    const oldAfter = previous.after[path]?.id;
    const displayed = after ?? before;
    if (!displayed || displayed !== (oldAfter ?? oldBefore)) continue;
    const sameDiff = diff && before === oldBefore && after === oldAfter;
    visits.push([JSON.stringify({ visited: { path, diff: Boolean(sameDiff) } }), true]);
  }
  return visits;
}

export async function rememberExploration(track, scope, snapshot) {
  const [previous] = await readReview(track);
  if (previous?.scope === scope) return;
  if (previous) {
    const visits = carryVisits(await readProgress(previous.scope), previous, snapshot);
    if (visits.length) await writeProgress(scope, visits);
  }
  await saveReview(track, "latest", { scope, before: snapshot.before, after: snapshot.after });
}
