# Misc runtime semantics: dates, globs, regex, Jira, cache, metrics

Evidence: `rg` over `lib/`, installed sources in `node_modules/`, and probe scripts run on Node 22.23.5 (ICU 78.3,
Unicode 17, TZ unset = Europe/Vienna on the probe host) and a Rust probe crate (chrono 0.4.45, chrono-tz 0.10.4,
globset 0.4.21, regex 1.13.2, fancy-regex 0.20.2, prometheus-client 0.26.0, moka 0.12.16).

## 1. Dates (moment-timezone 0.5.38 / moment 2.29.4, `Date`)
Every use (`rg "moment|new Date|toLocaleString|Date.parse" lib`):
| site | code | Rust |
|---|---|---|
| `lib/actions/handlebars/populateTemplate.js:15-28` | `formatDate` helper | see below |
| `lib/actions/checks.js:62,117`, `lib/flex/lib/processWorkflow.js:33,69`, `lib/flex/lib/logAndProcessConfigErrors.js:23` | `completed_at`/`started_at: new Date()` -> JSON `"2024-01-15T10:00:00.000Z"` | `Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)` (probe: `2024-01-15T10:00:00.000Z`) |
| `lib/validators/age.js:6-9,41` | `moment().utc(false).diff(moment(ts))` -> `moment.duration(diff).asDays() >= days` | `(now - parsed).num_milliseconds() as f64 / 86_400_000.0` (probe: 4.5 days for 10:00Z vs 22:00Z five days earlier) |
| `lib/validators/stale.js:44-50` | `now.tz(time_zone)`, `now.day()` (0 = Sunday), `now.hour()` | `with_timezone(&tz)`, `weekday().num_days_from_sunday()`, `hour()` (probe 23:30Z in Asia/Tokyo: 2 / 8 both) |
| `lib/validators/stale.js:66-68` | `new Date(new Date() - days*864e5).toISOString().replace(/\.\d{3}\w$/, '')` | `format("%Y-%m-%dT%H:%M:%S")` (probe: `2024-01-15T10:00:00`, no zone) |

moment traps: `moment(undefined)` is *now* (diff 0 -> age 0 days); `moment('garbage')` is invalid -> diff `NaN`,
`NaN >= days` is false -> "fail" (plus a deprecation warning, probe). Unknown `time_zone`: moment-timezone logs
"Moment Timezone has no data for Not/AZone" and **keeps UTC** (probe: day 1, hour 23); `chrono_tz::Tz::from_str` returns
`Err(ParseError)` -> fall back to UTC, do not fail.

### `formatDate` (`populateTemplate.js:15-28`)
```js
let date = new Date(); if (str === undefined) return str
if (typeof str === 'string') { try { date = new Date(str) } catch { return str } }   // new Date never throws
return date.toLocaleString('en-US', { dateStyle: 'medium', timeStyle: 'short', timeZone: 'UTC' })
```
- Output format (probe): `Jan 15, 2024, 10:00 AM`, `Jan 15, 2024, 12:05 AM`, `Jan 15, 2024, 12:00 PM`, `Jul 4, 2024, 11:59 PM`.
  **The space before AM/PM is U+0020** in this Node 22 / ICU 78.3 build (the probe escaped every non-ASCII char; none
  appeared) -- no U+202F, and the separators are `, ` (ASCII). chrono `format("%b %-d, %Y, %-I:%M %p")` produced
  identical strings for the same instants (probe). Re-check if the reference Node version changes.
- `undefined` (missing field) -> returns `undefined` -> renders ``. **Any non-string** (null, number, object) -> formats
  **the current time** (`date` stays `new Date()`).
- Invalid string -> `"Invalid Date"` (probe: `"not a date"`, `""`, `"20240115"`, `"1705312800000"`).
- `new Date(string)` (V8) semantics by probe (host TZ +01:00):
  - ISO with `Z`/offset -> exact; fractional seconds beyond ms truncated (`.123456Z` -> `.123Z`).
  - date-only ISO `2024-01-15` -> **UTC** midnight.
  - date-time without offset `2024-01-15T10:00:00` / `2024-01-15 10:00` -> **local time** (rendered `9:00 AM` UTC here).
  - legacy formats `Jan 15 2024`, `1/15/2024`, `2024-1-5`, `"0"` (= year 2000) -> local time; RFC 2822 with `GMT` -> exact.
  - overflow `2024-02-30` -> **Mar 1, 2024** (chrono rejects it).
  - extended years `-000001-...` / `+275760-...` accepted.
  chrono: `DateTime::parse_from_rfc3339` handles the GitHub `...Z` timestamps (probe) but not date-only or offset-less
  forms; `parse_from_rfc2822` handles `Mon, 15 Jan 2024 10:00:00 GMT`. Port: rfc3339 -> date-only (UTC) -> offset-less
  (interpret in `TZ`, i.e. `chrono::Local`) -> rfc2822 -> `"Invalid Date"`; day overflow and legacy formats are known diffs.

## 2. Globs: minimatch 3.1.2 vs globset
Uses: `lib/actions/labels.js:6` `minimatch(label, pattern)`; `lib/validators/size.js:258` `{ dot: true }`;
`lib/validators/dependent.js:52` and `lib/validators/contents.js:87` no options. Defaults: `dot: false`, case-sensitive,
braces, extglob, `!` negation, `#` comment (`node_modules/minimatch/minimatch.js:115-165, 219, 293-324`).
| file / pattern | minimatch | globset (`literal_separator(true)`, `backslash_escape(true)`) |
|---|---|---|
| `.github/x.yml` / `**/*.yml` | **false** (dot) ; `{dot:true}` true | true |
| `.env` / `*` | **false**; dot: true | true |
| `a/.b/c` / `a/**/c` | false; dot: true | (globset has no dot rule) |
| `bug` / `!feat*` | **true** (negation) | false |
| `#x` / `#x` | **false** (comment) | true |
| `aab` / `+(a)b` | true (extglob) | false |
| `a1` / `a{1..3}` | true (brace range) | false |
| `a/b` / `a//b` | **true** (empty segments collapse) | false |
| `a/b` / `a/**/` | false | - |
| `ab` / `a[!c]`, `a[^c]` | true / true | true / true |
| `src/x/a.js` / `src/*.js` | false | false |
| `src/a.js` / `src/**/*.js`, `c.js` / `**/c.js`, `a/b` / `a/**` | true | true |
| `a` / `a/**` | false | false |
| `a.JS` / `*.js` | false | false |
| `a/b` / `b` (no matchBase) | false | false |
| `x/y` / `x\\y` | false (backslash escapes y) | - |
globset cannot express minimatch's dotfile rule, negation, comments, extglobs or ranges. Port minimatch 3's
`makeRe` (pattern -> JS regex source incl. the `(?!\.)` dot guards, `minimatch.js:324, 646-690`) and run the result with
fancy-regex (the guards are lookaheads), or hand-port `minimatch.match` (`minimatch.js:738-777`, segment matcher 843-868).

## 3. Regular expressions
User-supplied: `must_include.js:24-30`, `must_exclude.js:24-30`, `jira.js:21-27`: `new RegExp(regex, flag)`, flag
default **`'i'`**, `regex_flag: none` -> `''`, else the string verbatim (any of `dgimsuvy`; invalid flags throw ->
"Failed to create a regex expression with the provided regex: <regex>"). `regex` may be a list (`[].concat(regex)`) and
non-strings are stringified (YAML `123` -> `/123/`). Fixed: `lib/validators/project.js:40` (project name as regex, `i`),
`gitPattern.js:2-90` (`i`), `deepValidation.js:2` (`ig`), `size.js:194-246`, `listProcessor.js:24`, and
`searchAndReplaceSpecialAnnotation.js:13-15` which uses **lookbehind** `(?<!\\)@author`.
Probe matrix (JS / regex crate / fancy-regex):
| pattern, input | JS (no `u`) | regex / fancy |
|---|---|---|
| `s` + `i` vs U+017F (long s) | false | **true** (Unicode simple case folding) |
| `k` + `i` vs U+212A (Kelvin) | false (`iu`: true) | **true** |
| `\w+$` vs `café` | false (ASCII `\w`) | **true** |
| `^\d$` vs U+0663 | false | **true** |
| `\bfix\b` + `i` vs `éfix` | true (ASCII boundary) | false; `(?-u:\b)` -> true in regex, **error in fancy-regex** ("Changing Unicode mode inline is not supported") |
| `^a.b$` vs `a\rb`, `a<U+2028>b` | false, false | **true, true** (`.` excludes only `\n`) |
| `^b$` + `m` vs `a\rb`, `a<U+2028>b` | true, true | false, false |
| `\s` vs U+FEFF / U+0085 | true / false | false / true |
| `[^]` | any char | parse error |
| `(?<!\\)@author`, `feat(?=:)`, `(a)\1`, `\k<name>` | ok | regex: error; fancy: ok |
| `a{,2}` | literal `a{,2}` | regex: error; fancy: quantifier (matches `a`) |
| `\p{L}` (no `u`) | literal `p{L}` | Unicode class |
| `\z` | literal `z` | end-of-text assertion |
| `[\w-.]` | ok (`-` literal) | error "invalid range boundary" |
| `\/`, `\x` escapes like `\u` + 4 hex | ok | ok |
Port: compile every JS pattern through a translator: `\d` -> `[0-9]`, `\w` -> `[0-9A-Za-z_]`, `\W`/`\D` likewise,
`\b`/`\B` -> ASCII-word lookarounds (fancy-regex), `.` -> `[^\n\r\u{2028}\u{2029}]` (or with `s` any), `\s` -> JS
WhiteSpace+LineTerminator set, `[^]` -> `[\s\S]`, identity escapes (`\z`, `\p` without `u`) -> literal, `{,n}` -> literal,
`-` after a class escape inside `[]` -> literal. Case-insensitive without `u`: JS canonicalizes with `toUpperCase` and
refuses to map non-ASCII to ASCII (U+017F, U+212A, `ß` stay distinct); emulate by replacing `(?i)` with explicit
`[xX]` classes for ASCII letters and simple case pairs, or accept known diffs for exotic code points.
Flag `g`/`y`: `RegExp.prototype.test` is **stateful** (`lastIndex`); one regex object is reused across array items in
must_include/must_exclude, probe `/bug/gi` on `['bug','bug','bug']` -> `[true, false, true]`. Emulate `lastIndex` when the
flag string contains `g` or `y`.
Replacement strings: `String.prototype.replace(regex, str)` expands `$&`, `$1`, `$$`...: probe `'hi @bot'.replace(/@bot/, 'A$&B')`
-> `hi A@botB` (APP_NAME flows into `@bot`). Without `g` only the **first** occurrence is replaced
(`x me y @author`): `searchAndReplaceSpecialAnnotations` replaces each annotation at most once per pass. Use
`fancy_regex::Regex::replace` (first match) with a JS-`$`-expansion function, not `replace_all`.
Latent JS bug to keep: `owners.js:59` calls `EMAIL_REGEX.test(owner)` on a **string** (`owners.js:99`) -> `TypeError:
EMAIL_REGEX.test is not a function` for every CODEOWNERS entry that does not start with `@`.

## 4. Jira (`lib/validators/options_processor/options/jira.js`, jira-client 6.23.0)
- Only the first match of the regex in the input is used (`regexObj.exec(input)[0]`), then `findIssue(id)`.
- Request (`node_modules/jira-client/lib/jira.js:150-195, 385-397, 330-371`): `GET {JIRA_PROTOCOL||https}://{JIRA_HOST}/rest/api/{JIRA_VERSION||2}/issue/{id}?expand=&fields=*all&properties=*all&fieldsByKeys=false`
  (probe of `url.format` + `decodeURIComponent`), basic auth `JIRA_USERNAME:JIRA_PASSWORD` (request `auth.user/pass`),
  `json: true` (`accept: application/json`), `rejectUnauthorized: strictSSL`.
- `JIRA_HOST` with a port (`jira.example.com:8443`) becomes `https://[jira.example.com:8443]/...` (url.format treats it as
  IPv6) -> request fails -> ticket invalid. `JIRA_STRICT_SSL` is passed as a string; `"false"` is truthy and TLS
  checks `rejectUnauthorized !== false`, so verification stays on.
- Outcome: any 2xx -> pass; non-2xx (request-promise `simple: true`), network error, or a body with non-empty
  `errorMessages` -> fail with `filter.message || 'The JIRA TICKET is not valid'`; success text `The JIRA TICKET is valid`.
- reqwest: `client.get(url).basic_auth(user, Some(password)).header(ACCEPT, "application/json")`, treat every error as
  "not valid". Query parameter order as above.

## 5. Cache (cache-manager 3.6.3 + lru-cache 6.0.0, cache-manager-ioredis 2.1.0) -- `lib/cache/cache.js`
- Memory store (`cache-manager/lib/stores/memory.js:25-74`): `max: args.max || 500`, `maxAge = ttl * 1000` when ttl set
  (seconds; the env **string** works because `"60" * 1000`), no ttl -> never expires; values deep-cloned on set
  (probe: mutation after `set` not visible). LRU eviction (probe max 2: keys `['c','b']`). `keys()` is MRU-first and
  still lists expired entries (probe after ttl: `['o','c']` while `get('c')` is undefined); `del` resolves undefined.
- **`CACHE_MEMORY_MAX` set in env crashes**: the value is a string and lru-cache 6 throws `TypeError: max must be a
  non-negative number` (probe). Only the unset case (500) works in Node.
- Three **separate** cache instances (`new CacheManager()` at module load in `settings.js:7`, `configuration.js:7`,
  `filters/options_processor/topics.js:6`): with the memory store they never share entries; with Redis they share one
  keyspace. Keys: `<owner>/<repo>` (config, `configuration.js:160,184`), `<owner>/<repo>/settings` (`settings.js:90,128`),
  `<owner>/<repo>/topics` (`topics.js:21,33`). `resetCache` deletes `<owner>/<repo>` (or `/settings`), or, for the
  `.github` repo, every key starting with `<owner>/` via `keys()` (`configuration.js:110-113`, `settings.js:72-75`) -- in
  Redis that also wipes the other modules' entries; `Configuration.resetCache` is called without `await`.
- Redis store (`cache-manager-ioredis/dist/index.js:25-109`): key stored verbatim (no prefix), value `JSON.stringify(v)`,
  `SETEX key ttl` when a ttl is configured else `SET`, `GET` + `JSON.parse`, `keys()` = `KEYS *`, `del` = `DEL`;
  `null`/`undefined` are not cacheable. **Bug to keep or fix knowingly**: `password: process.env.CACHE_REDIS_HOST`
  (`cache.js:10`). Note JSON round trip turns YAML Dates into strings for cached configs.
- moka: `Cache::builder().max_capacity(500).time_to_live(Duration::from_secs(ttl))` (`moka-0.12.16/src/future/builder.rs:245,370`);
  `iter()` order is arbitrary (`future/cache.rs:1431`), which only matters for `keys()` prefix deletion (order-insensitive).
  Clone values on insert (`Value` clone) to mirror cloneDeep.

## 6. Metrics (prom-client 13.2.0, express-prometheus-middleware 1.2.0)
- Gauges (`lib/stats/githubPrometheusStats.js`): `github_ratelimit_remaining_total`, `github_ratelimit_limit_total`, label
  `installationId`, set after **every** Octokit response from `x-ratelimit-remaining`/`x-ratelimit-limit`
  (`lib/mergeable.js:54-66`); a missing header gives `Number(undefined)` = NaN, printed `Nan` by prom-client (probe).
- Middleware (`node_modules/express-prometheus-middleware/src/index.js:17-32, 68-114, 140-157`; `metrics.js`;
  `normalizers.js`): `GET /metrics` -> `text/plain; version=0.0.4; charset=utf-8`; counters/histograms
  `http_requests_total`, `http_request_duration_seconds` (buckets `exponentialBuckets(0.05, 1.75, 8)`),
  `http_request_length_bytes`, `http_response_length_bytes` (no buckets -> never observed), labels
  `route` (ids masked to `#val` by url-value-parser), `method`, `status` (`2XX`..`5XX`); plus prom-client default
  `process_*` and `nodejs_*` metrics. Because it is mounted as `router.use` after the webhook middleware and the
  rate-limit route (`index.js:6-19`), only requests that fall through (unknown paths, probe: `/nope/#val` 4XX) are counted;
  webhooks are never counted.
- prometheus-client 0.26 encodes **OpenMetrics** (`# EOF` terminator, `src/encoding/text.rs:273`; examples use
  `application/openmetrics-text; version=1.0.0; charset=utf-8`) and appends `_total` to counters itself (register
  `http_requests`, not `http_requests_total`). NaN prints `NaN`. `nodejs_*` metrics have no Rust equivalent; `process_*`
  need a separate collector.

## Traps for the port
- [ ] `formatDate`: `Jan 15, 2024, 10:00 AM` with ASCII spaces (checked on Node 22.23.5/ICU 78.3); non-string arg -> *now*; invalid -> `Invalid Date`.
- [ ] JS date parsing: date-only = UTC, offset-less date-time = local TZ, `2024-02-30` rolls over; chrono needs a layered parser.
- [ ] age: fractional days from ms; unparsable date -> NaN -> fail; missing date -> now.
- [ ] stale: unknown time_zone keeps UTC; `day()` Sunday = 0; search timestamp without ms and without `Z`.
- [ ] minimatch: dotfiles excluded unless `dot` (only size.js sets it), `!` negation, `#` comments, extglob, ranges, `//` collapse -> do not use plain globset.
- [ ] Regex: JS non-Unicode `\w \d \b \s .`, case folding and identity escapes differ from Rust; lookbehind needs fancy-regex; default flag `i`; `none` = no flags.
- [ ] `g`/`y` flags make `.test` stateful across array items.
- [ ] `.replace` without `g` replaces only the first match and expands `$&`/`$n` in the replacement.
- [ ] `EMAIL_REGEX.test` TypeError in owners.js for non-`@` owners.
- [ ] Jira: exact URL/query, basic auth, any failure = invalid; host:port and JIRA_STRICT_SSL=false quirks.
- [ ] Cache: ttl seconds, max 500, clone on set, keys include expired entries; CACHE_MEMORY_MAX string crash; Redis password = host bug; Redis JSON values.
- [ ] Metrics: gauge names/label `installationId`, NaN when header missing, OpenMetrics vs 0.0.4 text and content type, counter `_total` suffix, only fall-through requests counted.
