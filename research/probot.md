# Probot 13.0.2 as installed: what the Rust server must reproduce

Versions (from `node_modules/*/package.json`): probot 13.0.2, @octokit/core 5.1.0, @octokit/request 8.2.0,
@octokit/endpoint 9.0.4, @octokit/plugin-paginate-rest 9.2.1, @octokit/plugin-retry 6.0.1, @octokit/plugin-throttling 8.2.0,
@octokit/request-error 5.0.1, @octokit/auth-app 6.0.4, universal-github-app-jwt 1.1.2, @octokit/webhooks 12.1.2,
@octokit/webhooks-methods 4.1.0, @probot/octokit-plugin-config 2.0.1 (with its own js-yaml **4.1.0**), deepmerge 4.3.1,
@probot/get-private-key 1.1.2. Paths below are relative to `node_modules/`. "Probe" = a Node 22 script run in devenv
against a local mock GitHub (results quoted verbatim).

## 1. Startup and env (`probot run ./index.js`, package.json `start`)
- `probot/lib/bin/read-env-options.js:5-33`: `APP_ID` -> `Number()`, `PORT` -> `Number(env.PORT) || 3000`, `HOST`,
  `WEBHOOK_SECRET`, `WEBHOOK_PATH`, `WEBHOOK_PROXY_URL`, `LOG_LEVEL`, `LOG_FORMAT`, `REDIS_URL` (throttling via Bottleneck
  clustering), base URL: `GHE_HOST ? \`${GHE_PROTOCOL || "https"}://${GHE_HOST}/api/v3\` : "https://api.github.com"` (line 29-31).
- `probot/lib/run.js:23` loads `.env` (dotenv) first. `run.js:61-89`: without APP_ID or private key, production throws,
  otherwise probot starts the **setup app** (manifest flow) instead of mergeable. Rust: fail fast with the same messages.
- Secret default: `probot/lib/probot.js:24` `options.secret || "development"` -- an unset WEBHOOK_SECRET means HMAC with key
  `development`, not "no verification".
- Private key (`@probot/get-private-key/dist-node/index.js`): `PRIVATE_KEY` (base64 decoded if it is base64; literal `\n`
  sequences replaced by newlines; must contain `-----BEGIN RSA PRIVATE KEY-----` (PKCS#1) and the END line, else throws),
  else `PRIVATE_KEY_PATH` (resolved from cwd, must exist), else the single `*.pem` in cwd (more than one -> error).
- Server (`probot/lib/server/server.js:35-58`): express; webhook middleware on `WEBHOOK_PATH || "/api/github/webhooks"`;
  `GET /ping` -> `200 PONG`; static `/probot/static/`; probot's default app also adds `GET /probot` and `GET /` -> redirect `/probot` (`apps/default.js:18-21`);
  `getRouter(path)` mounts express routers (mergeable: `/github-ratelimit/:installationId`, `/metrics`, `index.js:6-19`).
  Unknown routes: express default `404 text/html` `Cannot POST /nope`.

## 2. Webhook request handling (`@octokit/webhooks/dist-src/middleware/node/middleware.js`; runtime copy in dist-node)
| condition | status | body | source line |
|---|---|---|---|
| path matches, method != POST | 404, `content-type: application/json` | `{"error":"Unknown route: GET /api/github/webhooks"}` | 22-24, on-unhandled-request-default.js |
| content-type missing / not starting with `application/json` | 415 | `{"error":"Unsupported \"Content-Type\" header value. Must be \"application/json\""}` | 26-37 |
| missing `x-github-event`, `x-hub-signature-256`, `x-github-delivery` | 400 | `{"error":"Required headers missing: x-github-event, x-github-delivery"}` (names joined by `, ` in that order) | 38-49, get-missing-headers.js |
| bad signature | 400 | `{"error":"Error: [@octokit/webhooks] signature does not match event payload and secret"}` | verify-and-receive.js:9-16 |
| invalid JSON body | 400 | `{"error":"SyntaxError: Invalid JSON"}` (name of JSON.parse error + replaced message) | verify-and-receive.js:18-24 |
| handler resolves | 200 | `ok\n` (no content-type) | 71 |
| handler throws | `err.status ?? 500` of the **first** error | `{"error":"<err.name>: <err.message>"}` | 73-86 |
| handler still running after 9 s | 202 | `still processing\n`; handler keeps running | 54-59 |
Probes: `throw new Error('boom')` -> `500 {"error":"Error: boom"}`; a thrown Octokit `RequestError` 404 ->
**`404 {"error":"HttpError: Not Found - https://docs.github.com/rest"}`** (the GitHub status leaks into the webhook status);
TypeError -> `500 {"error":"TypeError: Cannot read properties of null (reading 'x')"}`; slow handler -> `202`.
Error responses set only `statusCode` (no content-type header). The response is sent **after** the handler finished: the
whole mergeable run (all GitHub calls) happens inside the request.

Signature (`@octokit/webhooks-methods/dist-src/node/verify.js:6-25`, `sign.js`, `utils.js:1-3`): algorithm chosen from the
header prefix (`sha256=` -> sha256, anything else -> **sha1**), HMAC over the raw UTF-8 body string, hex lowercase, compared
with `timingSafeEqual` after a length check. Probe: a `sha1=...` value in `x-hub-signature-256` is accepted (200).
`getPayload` (get-payload.js) reads the raw body (express has no body parser on that route).

Dispatch (`event-handler/receive.js:3-55`): hooks = `name.action`, `name`, `*`; mergeable registers only `onAny`
(`lib/mergeable.js:97`), so **every** event incl. `installation`, `ping` etc. runs the handler. Each handler first runs the
probot transform (`probot/lib/octokit/octokit-webhooks-transform.js:10-17`) which builds the installation Octokit
(`octokit-auth-probot/dist-src/auth.js:15-53`): no `installation` key -> unauthenticated Octokit whose requests throw
`Handling a "<event>.<action>" event: an "installation" key is missing...`; `installation.suspend/deleted` -> unauthenticated.
Errors are logged by `probot/lib/helpers/get-error-handler.js` and returned as above.

## 3. App JWT and installation token
- JWT (`universal-github-app-jwt/dist-node/index.js:21-32`): RS256, header `{"alg":"RS256","typ":"JWT"}`,
  `iat = now - 30`, `exp = iat + 600`, `iss = appId` as a **number** (probe: `{"iat":..,"exp":..,"iss":123}`,
  iat-now = -30, exp-now = 570).
- Token request (probe): `POST {baseUrl}/app/installations/{id}/access_tokens`, empty body, `authorization: bearer <jwt>`,
  `accept: application/vnd.github.v3+json`, `user-agent: probot/13.0.2 octokit-core.js/5.1.0 Node.js/22`.
  API calls then use `authorization: token ghs_...`.
- Cache: `probot/lib/probot.js:31-36` LRU, max 15000, ttl 59 min; key (`@octokit/auth-app/dist-node/index.js:133-147`) =
  `installationId` (+ repo ids / permissions, unused here). Probe: two `app.auth(42)` -> **1** token request.
- auth-app retries a request that gets **401** within 5 s of token creation, waiting 1 s, 2 s, ... (`auth-app ... index.js:407-429`).

## 4. Request pipeline: plugins that add or delay calls (`probot/lib/octokit/probot-octokit.js:14-39`)
Plugins: throttling, retry, paginate-rest, legacy REST methods, enterprise-compatibility, request logging, config.
- **Retry** (`@octokit/plugin-retry/dist-node/index.js:44-47, 88-92`): any status >= 400 except
  `[400, 401, 403, 404, 422, 451]` is retried 3 times, wait `(retryCount+1)^2` s (1 s, 4 s, 9 s). Probe: a 500 produced
  **4 identical GETs** and took 17 s. Network errors (status 500 synthesized) too. Replay: a stubbed 5xx yields 4 recorded calls.
- **Throttling** (`@octokit/plugin-throttling/dist-node/index.js:111-136, 47-66, 139-160`): Bottleneck groups are
  process-global (`id = "no-id"`): global maxConcurrent 10; **every non-GET/HEAD is serialized with minTime 1 s**; writes to
  "notification" paths (issues, issue comments, pulls, reviews, requested_reviewers, merge; list at lines 81-99) additionally
  spaced 3 s; `GET /search/*` spaced 2 s. Effects: writes are executed in FIFO submission order even when JS fires them via
  `Promise.all`, and a run with N writes takes >= N-1 seconds (benchmark!). 403 with `x-ratelimit-remaining: 0` -> waits until
  `x-ratelimit-reset`+1 s and retries up to 2 times (`get-octokit-throttle-options.js:12-19`); "secondary rate" 403 -> not
  retried (onSecondaryRateLimit returns undefined, lines 21-24).
- Logging plugin only logs.

## 5. URL building (@octokit/endpoint 9.0.4)
- Path params use RFC 6570 simple expansion -> `encodeURIComponent` + `!'()*` (`endpoint/dist-node/index.js:168-174`).
  **`/` in `{path}` is encoded as `%2F`**. Probe: `repos.getContent({path: '.github/mergeable.yml'})` ->
  `GET /api/v3/repos/o/r/contents/.github%2Fmergeable.yml`; `path: 'a b/ü#?.yml', ref: 'x y'` ->
  `/contents/a%20b%2F%C3%BC%23%3F.yml?ref=x%20y` (space as `%20` in query, not `+`).
- `q` for search: split on `+`, each part encodeURIComponent, joined with literal `+` (`index.js:129`).
- Unused params of a GET become query params; for POST/PATCH they become the JSON body (`endpoint` parse step).
- Rust: percent-encode path segments with the `encodeURIComponent` set (`percent-encoding` crate, `NON_ALPHANUMERIC` minus
  `-_.!~*'()` -- then additionally encode `!'()*`, i.e. keep only `A-Za-z0-9-_.~`).

## 6. Pagination (`@octokit/plugin-paginate-rest/dist-node/index.js:62-90`)
- **No `per_page` is added**; the first request is the endpoint exactly as merged (probe: `GET /repos/o/r/pulls/1/files`).
- Next URL = first match of `/<([^>]+)>;\s*rel="next"/` in the `link` header, requested **verbatim** (probe followed
  `/api/v3/repositories/9/pulls/1/files?per_page=100&page=2`), same method and headers. Stops when no next link.
- A 409 ends pagination with an empty page instead of throwing (lines 80-88). Results concatenated after the map fn.

## 7. Errors (`@octokit/request-error/dist-src/index.js:5-31`, `@octokit/request/dist-node/index.js:171-187`)
- `name = "HttpError"`, `status`, `String(error)` = `HttpError: <message>`. Message:
  `<data.message>` + (`: ` + `errors.map(JSON.stringify).join(", ")` if `errors` is an array) + (` - <documentation_url>` if present);
  string body -> body as is; no `message` -> `Unknown error: <JSON>`.
  Probes: `HttpError: Not Found - https://docs.github.com/rest`;
  `HttpError: Validation Failed: {"resource":"Issue","code":"invalid","field":"x"} - https://docs.github.com/rest/x`;
  500 without docs url -> `HttpError: Server Error`.
- mergeable embeds `${error}` in check text (`lib/configuration/configuration.js:203-204`), so this format is user-visible.

## 8. `context.repo()` (`probot/lib/context.js:45-55`)
`Object.assign({owner: payload.repository.owner.login, repo: payload.repository.name}, object)` -- caller keys override;
throws `context.repo() is not supported for this webhook event.` when `repository` is absent.

## 9. `context.config(fileName, defaultConfig)` (`probot/lib/context.js:153-168` + `@probot/octokit-plugin-config/dist-src`)
mergeable calls `context.probotContext.config(configPath, orgConfig)` (`lib/configuration/configuration.js:178`),
`configPath` default `mergeable.yml`, `orgConfig` is `null` unless `use_org_as_default_config`.
1. path = `path.posix.join(".github", fileName)`; no `ref` (default branch).
2. `getConfigFile` (`util/get-config-file.js:4-71`): extension must be json/yml/yaml (case-insensitive) else throws.
   Request `GET /repos/{owner}/{repo}/contents/{path}` with `mediaType.format = "raw"` -> `accept: application/vnd.github.v3.raw`.
   If the response `content-type` is exactly `application/json; charset=utf-8` -> throws "... exists, but is either a directory
   or a submodule" (**the replay mock must serve raw files with another content type**). Response body decoding
   (`@octokit/request/dist-node/index.js:161-170`): `application/json` -> JSON; missing content-type, `text/*` or ending in
   `charset=utf-8` -> text; **anything else -> ArrayBuffer**. Probe with `content-type: application/vnd.github.raw`: config
   became `["object ArrayBuffer"]` (js-yaml parsed `String(arrayBuffer)`). Real GitHub answers raw with
   `text/plain; charset=utf-8`-style types; the mock must do the same (probe with `text/plain; charset=utf-8` parsed correctly). 404 -> `config: null`. YAML is
   parsed with **js-yaml 4 `load`** (not mergeable's js-yaml 3 `safeLoad`), `load(data) || {}` (empty file -> `{}`);
   a string result or YAMLException -> plain `Error`: `[@probot/octokit-plugin-config] Configuration could not be parsed from
   <full url> (invalid YAML)` (`unsafe YAML` when the message contains "unknown tag"). That is not a `YAMLException`, so
   mergeable reports it as `Github API Error occurred while fetching the config file at .github/mergeable.yml \n Error from api:
   Error: [@probot/octokit-plugin-config] Configuration could not be parsed from http://HOST/api/v3/repos/o/r/contents/.github%2Fmergeable.yml (invalid YAML)`
   -- the URL contains baseUrl and `%2F`.
3. `getConfigFiles` (`util/get-config-files.js:4-56`): repo file; if its config is null and repo != `.github`, the same
   path in `{owner}/.github`. Then follow `_extends` of the **last** file only.
4. `_extends` regex (`util/extends-to-get-content-params.js:1-4`):
   `^(?:([a-z\d](?:[a-z\d]|-(?=[a-z\d])){0,38})/)?([-_.\w\d]+)(?::([-_./\w\d]+\.ya?ml))?$` (case-insensitive):
   `repo` -> same owner, same path; `owner/repo` -> same path; `owner/repo:path/x.yml` (path must end in .yml/.yaml).
   Non-string -> `Invalid value <json> for _extends in <url>`; no match -> `Invalid value "<v>" for _extends in <url>`.
   `_extends` is deleted from the loaded config. Chain continues while the next file has `_extends`; a target already loaded
   throws "Recursion detected. Ignoring  \"_extends: undefined\" ..." (the message reads `_extends` after deleting it).
   An extends target that 404s just ends the chain (config null, ignored).
5. Compose (`compose-config-get.js:10-13`): configs = file configs **reversed** (base first, repo last), falsy removed;
   `deepmerge.all([defaultConfig || {}, ...configs])` -- deepmerge default **concatenates arrays** (probe:
   `{mergeable:[{when:'a'}]}` + `{mergeable:[{when:'b'}]}` -> `[{when:'a'},{when:'b'}]`; nested objects merged).
   So with `use_org_as_default_config` or `_extends`, the `mergeable:` rule lists are concatenated (base/org rules first).
   End-to-end probe (repo `o/r` extends `base`, which extends `other/x:conf/a.yml`): requests in that order, result
   `{"z":1,"mergeable":[{"when":"x"},{"when":"base"},{"when":"b"}],"d":"2020-01-01T00:00:00.000Z","version":2}` --
   deepest base first, key order of the first config that introduced a key, YAML timestamp kept as a JS Date.
6. Returns `null` when no defaultConfig and no file had a non-null config (`context.js:164-166`); with a defaultConfig, the
   merged result even if no file exists.

mergeable's own reads (`lib/github/api.js:106-118`) use `repos.getContent` JSON and `Buffer.from(content, 'base64')`
(lenient: ignores the `\n` GitHub inserts every 60 chars and any non-alphabet byte) and map 404 -> `null`.

## Traps for the port
- [ ] Webhook answers only after the whole run; 9 s timeout -> `202 still processing\n` while work continues.
- [ ] Handler error status = `error.status` if set (a GitHub 404 bubbling up gives webhook 404), else 500; body `{"error":"Name: message"}`.
- [ ] 400/415/404 texts and header-missing order exactly as in the table; non-matching path -> express HTML 404.
- [ ] HMAC: sha256 unless the header lacks the `sha256=` prefix (then sha1); unset secret means key `development`.
- [ ] `onAny`: every event runs the engine; events without `installation` get an Octokit whose calls fail with probot's message.
- [ ] JWT iat = now-30, exp = iat+600, iss numeric; token cached 59 min per installation; retry 401 within 5 s of token creation.
- [ ] Retry plugin: 5xx (and other non-excluded statuses) retried 3x with 1/4/9 s backoff -> 4 calls per failing request.
- [ ] Throttling: writes serialized globally, >= 1 s apart (3 s for notification paths, 2 s for search) -> FIFO write order; timing for benchmarks.
- [ ] `{path}` encoding: `/` -> `%2F`, encodeURIComponent set plus `!'()*`; query spaces `%20`; `q` keeps literal `+`.
- [ ] paginate: no per_page added; follow `rel="next"` URL verbatim; 409 -> empty result.
- [ ] Error text `HttpError: msg[: errors-json, ...][ - docs_url]`, used inside check output.
- [ ] Replay mock: raw file responses need a `text/*` or `...charset=utf-8` content type, never exactly `application/json; charset=utf-8`.
- [ ] context.config: raw media type, js-yaml **4** `load`, empty file -> `{}`, `.github` repo fallback only on 404, `_extends` forms, recursion message, deepmerge array concatenation, `null` when nothing found.
- [ ] Config YAML parse errors through context.config are *API errors* in mergeable's classification, with the full API URL in the text.
- [ ] Base64 content decoding must be lenient (strip newlines), like `Buffer.from(x, 'base64')`.
- [ ] Private key: PKCS#1 marker required for PRIVATE_KEY, `\n` unescaping, base64 form, PRIVATE_KEY_PATH, single *.pem fallback.
