(function () {
  const globalScope = typeof window !== "undefined" ? window : globalThis;
  const STATE = {
    data: null,
    loading: null,
    base: null,
  };

  function resolveBaseUrl() {
    if (STATE.base) {
      return STATE.base;
    }
    const current =
      document.currentScript ||
      document.querySelector("script[data-thought-search]") ||
      Array.from(document.querySelectorAll("script[src]")).find((el) =>
        el.src.includes("thought-search.js")
      );
    if (current && current.src) {
      STATE.base = current.src.replace(/[^/]+$/, "");
    } else if (typeof window !== "undefined" && window.location) {
      const { origin, pathname } = window.location;
      STATE.base = `${origin}${pathname.replace(/[^/]+$/, "")}`;
    } else {
      STATE.base = "";
    }
    return STATE.base;
  }

  function normalize(text) {
    return text
      .toLocaleLowerCase()
      .normalize("NFKD")
      .replace(/[\u0300-\u036f]/g, "");
  }

  function unique(tokens) {
    return Array.from(new Set(tokens));
  }

  function tokenizeQuery(query) {
    const normalized = normalize(query);
    const words = normalized.split(/\s+/).filter(Boolean);
    if (words.length > 0 && words.some((token) => token.length > 1)) {
      return unique(words);
    }

    const chars = Array.from(normalized)
      .map((char) => char.trim())
      .filter(Boolean);
    return unique(chars);
  }

  function fuzzyScore(target, token) {
    if (!token) return 0;
    if (target.includes(token)) {
      return token.length * 2;
    }
    let ti = 0;
    for (let i = 0; i < target.length && ti < token.length; i += 1) {
      if (target[i] === token[ti]) {
        ti += 1;
      }
    }
    return ti === token.length ? token.length : 0;
  }

  function recordScore(record, tokens) {
    const haystacks = [record.title, record.description || "", record.permalink || ""]; 
    const normalized = haystacks.map(normalize);
    let score = 0;
    for (const token of tokens) {
      let best = 0;
      for (const value of normalized) {
        best = Math.max(best, fuzzyScore(value, token));
      }
      if (best === 0) {
        return 0;
      }
      score += best;
    }
    return score;
  }

  function normalizeLocale(locale) {
    return (locale || "").toLowerCase();
  }

  function recordKey(record) {
    const segments = Array.isArray(record.category)
      ? record.category.filter(Boolean)
      : [];
    const slug = record.slug || record.permalink || record.title || "";
    if (segments.length === 0) {
      return slug;
    }
    return `${segments.join("/")}/${slug}`;
  }

  function preferLocale(records, preferredLocale) {
    if (!Array.isArray(records) || records.length === 0) {
      return [];
    }
    const preferred = normalizeLocale(preferredLocale);
    const groups = new Map();
    const filtered = [];

    for (const record of records) {
      const key = recordKey(record);
      const existing = groups.get(key);
      const isPreferred = preferred && normalizeLocale(record.locale) === preferred;
      const isDefault =
        normalizeLocale(record.locale) === normalizeLocale(record.default_locale);

      if (!existing) {
        const index = filtered.length;
        filtered.push(record);
        groups.set(key, { record, index, isPreferred, isDefault });
        continue;
      }

      if (isPreferred || (!existing.isPreferred && isDefault)) {
        filtered[existing.index] = record;
        groups.set(key, { record, index: existing.index, isPreferred, isDefault });
      }
    }

    return filtered;
  }

  async function loadIndex() {
    if (STATE.loading) {
      return STATE.loading;
    }
    STATE.loading = (async () => {
      const base = resolveBaseUrl();
      const wasmUrl = `${base}thought-search.wasm`;
      const response = await fetch(wasmUrl);
      const buffer = await response.arrayBuffer();
      const { instance } = await WebAssembly.instantiate(buffer, {});
      const exports = instance.exports;
      const ptr = exports.thought_search_data_ptr();
      const len = exports.thought_search_data_len();
      const view = new Uint8Array(exports.memory.buffer, ptr, len);
      const json = new TextDecoder("utf-8").decode(view);
      STATE.data = JSON.parse(json);
      return STATE.data;
    })();
    return STATE.loading;
  }

  async function search(query, preferredLocale) {
    const trimmed = (query || "").trim();
    if (!trimmed) {
      return [];
    }
    const index = await loadIndex();
    const normalizedTokens = tokenizeQuery(trimmed);
    if (normalizedTokens.length === 0) {
      return [];
    }

    const scored = index
      .map((record) => ({
        record,
        score: recordScore(record, normalizedTokens),
      }))
      .filter((entry) => entry.score > 0)
      .sort((a, b) => b.score - a.score)
      .map((entry) => entry.record);

    return preferLocale(scored, preferredLocale);
  }

  globalScope.ThoughtSearch = {
    search,
    preferLocale,
    ready: () => loadIndex(),
  };
})();
