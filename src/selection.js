// @ts-check
// Multi-select rules shared by every list in the app (Apps, Tree,
// All processes, Startup). Pure functions, so they can be unit tested with
// `node --test`. Loading this file defines globalThis.MultiSelect (a plain
// <script> in the app, an import in tests).
"use strict";

(function (root) {
  /**
   * @template K
   * @typedef {{ keys: Set<K>, focus: K | null, anchor: K | null }} Sel
   */

  /** @template K @returns {Sel<K>} */
  function empty() {
    return { keys: new Set(), focus: null, anchor: null };
  }

  /**
   * Apply a click (or arrow key) to a selection.
   * - plain: select just `key`
   * - toggle (⌘-click): add or remove `key`, keep the rest
   * - extend (⇧-click / ⇧-arrow): select the range from the anchor to `key`,
   *   added to what ⌘-click had already picked
   * @template K
   * @param {Sel<K>} sel
   * @param {K[]} rows keys in display order
   * @param {K} key
   * @param {{ toggle?: boolean, extend?: boolean }} [mods]
   * @returns {Sel<K>}
   */
  function click(sel, rows, key, mods = {}) {
    if (mods.extend && sel.anchor != null && rows.includes(sel.anchor) && rows.includes(key)) {
      const a = rows.indexOf(sel.anchor);
      const b = rows.indexOf(key);
      const [lo, hi] = a < b ? [a, b] : [b, a];
      const keys = new Set(mods.toggle ? sel.keys : []);
      for (const k of rows.slice(lo, hi + 1)) keys.add(k);
      return { keys, focus: key, anchor: sel.anchor };
    }
    if (mods.toggle) {
      const keys = new Set(sel.keys);
      if (keys.has(key)) {
        keys.delete(key);
        const focus = sel.focus === key ? (last(keys) ?? null) : sel.focus;
        return { keys, focus, anchor: focus };
      }
      keys.add(key);
      return { keys, focus: key, anchor: key };
    }
    return { keys: new Set([key]), focus: key, anchor: key };
  }

  /**
   * Move the focus one row up or down (arrow keys), optionally extending.
   * @template K
   * @param {Sel<K>} sel @param {K[]} rows @param {number} delta
   * @param {boolean} [extend]
   * @returns {Sel<K>}
   */
  function step(sel, rows, delta, extend = false) {
    if (!rows.length) return sel;
    const i = sel.focus != null ? rows.indexOf(sel.focus) : -1;
    const j = i < 0 ? (delta > 0 ? 0 : rows.length - 1) : Math.max(0, Math.min(rows.length - 1, i + delta));
    return click(sel, rows, rows[j], { extend });
  }

  /** @template K @param {K[]} rows @returns {Sel<K>} */
  function all(rows) {
    return { keys: new Set(rows), focus: rows[0] ?? null, anchor: rows[0] ?? null };
  }

  /**
   * Drop keys that no longer exist (process ended, item removed).
   * @template K
   * @param {Sel<K>} sel @param {(k: K) => boolean} alive
   * @returns {Sel<K>}
   */
  function prune(sel, alive) {
    const keys = new Set([...sel.keys].filter(alive));
    if (keys.size === sel.keys.size) return sel;
    const focus = sel.focus != null && keys.has(sel.focus) ? sel.focus : (last(keys) ?? null);
    const anchor = sel.anchor != null && keys.has(sel.anchor) ? sel.anchor : focus;
    return { keys, focus, anchor };
  }

  /** @template K @param {Set<K>} s @returns {K | undefined} */
  function last(s) {
    let v;
    for (v of s);
    return v;
  }

  root.MultiSelect = Object.freeze({ empty, click, step, all, prune });
})(globalThis);
