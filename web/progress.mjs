const table = "marks";
const reviewTable = "review";
let open;

function database() {
  open ??= new Promise((resolve, reject) => {
    const request = indexedDB.open("repo-city-progress", 2);
    request.onupgradeneeded = event => {
      if (event.oldVersion === 0) request.result.createObjectStore(table);
      request.result.createObjectStore(reviewTable);
    };
    request.onerror = () => reject(request.error);
    request.onsuccess = () => resolve(request.result);
  }).catch(error => {
    open = undefined;
    throw error;
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
