# Sweep website

Deployed with **Cloudflare Pages** to https://sweep.subultech.top.

```
web/
├── public/            static site (index.html, styles.css, app.js, llms.txt, robots.txt, sitemap.xml, _headers)
└── functions/
    └── api/latest.js  GET /api/latest  → latest GitHub release as JSON
```

## Cloudflare Pages settings

| Setting | Value |
|---|---|
| Production branch | `main` |
| Root directory | `web` |
| Build command | *(empty)* |
| Build output directory | `public` |
| Environment variable (optional) | `GITHUB_TOKEN`: a fine-grained token with read-only access to public repos, to raise GitHub's rate limit |

Pages picks up `web/functions/` automatically.

## `GET /api/latest`

Returns the latest **published** release (drafts and pre-releases are ignored). Cached at the edge for 10 minutes. CORS is open (`*`).

```json
{
  "version": "0.2.0",
  "tag": "v0.2.0",
  "name": "Sweep v0.2.0",
  "published_at": "2026-10-19T08:00:00Z",
  "notes": "release notes (markdown)",
  "release_url": "https://github.com/abdulrasol/sweep/releases/tag/v0.2.0",
  "downloads": {
    "macos": { "url": "https://github.com/.../Sweep_0.2.0_universal.dmg", "name": "Sweep_0.2.0_universal.dmg", "size": 12345678 }
  }
}
```

Add `?current=0.1.0` to also get `"current"` and `"update_available": true|false`.

Errors: `404 {"error":"no_published_release"}`, `400 {"error":"invalid_current_version"}`, `502 {"error":"github_error"|"github_unreachable"}`.

## Preview locally

```bash
cd web
npx wrangler pages dev public
```

Run it from `web/` so Wrangler also loads `functions/`.
