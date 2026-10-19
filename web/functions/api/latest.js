// GET /api/latest
//
// Returns the latest published Sweep release from GitHub as a small, stable JSON
// document, so the website and the app don't depend on GitHub's API shape or rate
// limits. Cached at the edge for CACHE_SECONDS.
//
// Optional query:  ?current=0.1.0   adds "update_available" (true/false).
// Optional env:    GITHUB_TOKEN     raises GitHub's rate limit (Pages > Settings > Variables).
//
// Response 200:
// {
//   "version": "0.2.0",
//   "tag": "v0.2.0",
//   "name": "Sweep v0.2.0",
//   "published_at": "2026-10-19T08:00:00Z",
//   "notes": "…release notes (markdown)…",
//   "release_url": "https://github.com/abdulrasol/sweep/releases/tag/v0.2.0",
//   "downloads": {
//     "macos": { "url": "https://…/Sweep_0.2.0_universal.dmg", "name": "Sweep_0.2.0_universal.dmg", "size": 12345678 }
//   },
//   "update_available": true          // only when ?current= is given
// }

const REPO = "abdulrasol/sweep";
const CACHE_SECONDS = 600;

const CORS = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Methods": "GET, OPTIONS",
  "Access-Control-Allow-Headers": "Content-Type",
};

function json(body, status, extra) {
  return new Response(JSON.stringify(body, null, 2), {
    status,
    headers: {
      "Content-Type": "application/json; charset=utf-8",
      ...CORS,
      ...extra,
    },
  });
}

// Parse "1.2.3" or "v1.2.3" (pre-release suffix ignored). Returns null if invalid.
export function parseVersion(v) {
  const m = String(v || "").trim().replace(/^v/i, "").match(/^(\d+)\.(\d+)\.(\d+)/);
  return m ? [Number(m[1]), Number(m[2]), Number(m[3])] : null;
}

export function isNewer(latest, current) {
  const a = parseVersion(latest);
  const b = parseVersion(current);
  if (!a || !b) return null;
  for (let i = 0; i < 3; i++) {
    if (a[i] !== b[i]) return a[i] > b[i];
  }
  return false;
}

function pickAsset(assets, pattern) {
  return assets.find((a) => pattern.test(a.name)) || null;
}

function asset(a) {
  return a ? { url: a.browser_download_url, name: a.name, size: a.size } : null;
}

export function shapeRelease(rel) {
  const assets = Array.isArray(rel.assets) ? rel.assets : [];
  const dmg =
    pickAsset(assets, /universal.*\.dmg$/i) || pickAsset(assets, /\.dmg$/i);
  const msi = pickAsset(assets, /\.msi$/i) || pickAsset(assets, /setup.*\.exe$/i);
  const appimage = pickAsset(assets, /\.AppImage$/i);
  const deb = pickAsset(assets, /\.deb$/i);

  const downloads = {};
  if (dmg) downloads.macos = asset(dmg);
  if (msi) downloads.windows = asset(msi);
  if (appimage) downloads.linux_appimage = asset(appimage);
  if (deb) downloads.linux_deb = asset(deb);

  const tag = rel.tag_name || "";
  return {
    version: tag.replace(/^v/i, ""),
    tag,
    name: rel.name || tag,
    published_at: rel.published_at || null,
    notes: rel.body || "",
    release_url: rel.html_url || `https://github.com/${REPO}/releases/latest`,
    downloads,
  };
}

export async function onRequestOptions() {
  return new Response(null, { status: 204, headers: CORS });
}

export async function onRequestGet(context) {
  const { request, env } = context;
  const url = new URL(request.url);
  const current = url.searchParams.get("current");

  // Cache the GitHub response per edge location, independent of ?current=.
  const cache = typeof caches !== "undefined" ? caches.default : null;
  const cacheKey = new Request(`${url.origin}/api/latest`, { method: "GET" });

  let data = null;
  if (cache) {
    const hit = await cache.match(cacheKey);
    if (hit) data = await hit.json();
  }

  if (!data) {
    const headers = {
      Accept: "application/vnd.github+json",
      "User-Agent": "sweep-website",
      "X-GitHub-Api-Version": "2022-11-28",
    };
    if (env && env.GITHUB_TOKEN) headers.Authorization = `Bearer ${env.GITHUB_TOKEN}`;

    let res;
    try {
      res = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`, { headers });
    } catch (e) {
      return json({ error: "github_unreachable" }, 502, { "Cache-Control": "no-store" });
    }
    if (res.status === 404) {
      return json({ error: "no_published_release" }, 404, { "Cache-Control": "public, max-age=60" });
    }
    if (!res.ok) {
      return json({ error: "github_error", status: res.status }, 502, { "Cache-Control": "no-store" });
    }

    data = shapeRelease(await res.json());

    if (cache) {
      const toCache = new Response(JSON.stringify(data), {
        headers: {
          "Content-Type": "application/json",
          "Cache-Control": `public, max-age=${CACHE_SECONDS}`,
        },
      });
      context.waitUntil(cache.put(cacheKey, toCache));
    }
  }

  const body = { ...data };
  if (current !== null) {
    const newer = isNewer(data.version, current);
    if (newer === null) {
      return json({ error: "invalid_current_version", current }, 400, { "Cache-Control": "no-store" });
    }
    body.current = current.replace(/^v/i, "");
    body.update_available = newer;
  }

  return json(body, 200, { "Cache-Control": `public, max-age=${CACHE_SECONDS}` });
}
