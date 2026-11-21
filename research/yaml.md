# js-yaml (3.14.1 `safeLoad` and 4.1.0 `load`) vs serde_yaml_ng 0.10.0

Two YAML loaders are live in the Node app:
- **js-yaml 3.14.1 `safeLoad`** (`DEFAULT_SAFE_SCHEMA`) in mergeable itself: settings file (`lib/settings/settings.js:113`),
  PR-branch config (`lib/configuration/configuration.js:149`), org default config (`configuration.js:175`).
- **js-yaml 4.1.0 `load`** (`node_modules/@probot/octokit-plugin-config/node_modules/js-yaml`) inside probot's
  `context.config` for the normal config path (`@probot/octokit-plugin-config/dist-src/util/get-config-file.js:49`).
  So the same `.github/mergeable.yml` is parsed by v3 when it changes in a same-repo PR and by v4 otherwise.
Schema of v3 safeLoad: core + implicit `timestamp`, `merge` + explicit `!!binary`, `!!omap`, `!!pairs`, `!!set`
(`node_modules/js-yaml/lib/js-yaml/schema/default_safe.js:14-28`). v4 `load` uses the same set by default (js-yaml 4 dropped
the unsafe `!!js/*` types from the default schema).

All rows below are probe output: Node 22 (`JSON.stringify` with Dates shown as `DATE(iso)`, non-finite as `NUM:x`) vs a Rust
probe deserializing into `serde_yaml_ng::Value` and `serde_json::Value`.

## Scalars
| input | js-yaml 3 safeLoad | js-yaml 4 load | serde_yaml_ng |
|---|---|---|---|
| `yes no on off y n` | strings | strings | strings |
| `true True TRUE false FALSE` / `tRue` | bool / string | same | same |
| `~ null Null NULL` (empty) | null | null | null |
| `0o17` | **"0o17"** (string) | 15 | 15 |
| `017` | **15** (YAML 1.1 octal) | 17 | **"017"** (string) |
| `0x1F` / `0b101` / `+1` / `-0` | 31 / 5 / 1 / 0 | same | same |
| `1_000` | **1000** | 1000 | **"1_000"** |
| `190:20:30` (sexagesimal) | **685230** | "190:20:30" | "190:20:30" |
| `08` | "08" | **8** | "08" |
| `12345678901234567890` | 12345678901234567000 (f64) | same | exact u64 |
| `9007199254740993` | 9007199254740992 (f64) | same | exact |
| `1.0` / `1e3` | 1 / 1000 (JS number) | same | f64 `1.0` / `1000.0` (renders `1.0`!) |
| `.inf -.Inf .NaN .nan` | Infinity / -Infinity / NaN | same | f64 inf/nan; **`null`** after conversion to serde_json |
| `1_000.5` | 1000.5 | 1000.5 | "1_000.5" |
| `2020-01-01` | **Date** 2020-01-01T00:00:00Z | Date | **string** |
| `2020-01-01T10:00:00Z`, `2001-12-14 21:59:43.10 -5` | Date (UTC computed, `type/timestamp.js:24-71`) | Date | string |
| `"2020-01-01"` (quoted) | string | string | string |

js-yaml int resolution strips `_` (`lib/js-yaml/type/int.js:110`) and supports `:` base-60 (line 129); v3 treats a leading
`0` as octal. Timestamps: `YAML_DATE_REGEXP` / `YAML_TIMESTAMP_REGEXP` (`type/timestamp.js:5-25`), constructed with
`Date.UTC` (lines 45, 71). A Date flows on as an object: `JSON.stringify` -> ISO string; handlebars `{{x}}` -> `String(date)`
= **local-timezone** text, probe: `Wed Jan 01 2020 01:00:00 GMT+0100 (Central European Standard Time)`; `typeof` -> "object".
(deepmerge in probot keeps Dates as values, see probot.md.)

## Structure
| case | js-yaml 3 | js-yaml 4 | serde_yaml_ng |
|---|---|---|---|
| merge key `<<: *b` then `y: 3` | `{"x":1,"y":3}` | same | **not applied** by `from_str`: key `"<<"` stays (also when deserializing into `serde_json::Value`); `Value::apply_merge()` gives `{"y":3,"x":1}` (**merged keys appended, order differs**) |
| `<<: [*a, *b]` | `{"x":1,"z":2}` (first wins) | same | after `apply_merge`: `{"x":1,"z":2}` |
| anchors/aliases | copied | same | copied |
| duplicate key | error `duplicated mapping key at line 2, column 1:\n    a: 2\n    ^` | error `duplicated mapping key (2:1)\n\n 1 \| a: 1\n 2 \| a: 2\n-----^` | error `duplicate entry with key "a"` |
| two documents | error `expected a single document in the stream, but found more` (`loader.js:1622`) | same | error `deserializing from YAML containing more than one document is not supported` |
| `---\na: 1\n...` | `{a:1}` | same | same |
| non-string keys `1:`, `true:`, `null:`, `1.5:` | string keys "1","true","null","1.5" | same | `Value` keeps typed keys; into serde_json: same strings |
| complex key `? [a, b]` | key `"a,b"` | same | serde_json conversion **error** |
| `!!binary aGVsbG8=` | Buffer -> `{"type":"Buffer","data":[...]}` | Uint8Array -> `{"0":104,...}` | string `"aGVsbG8="` |
| `!!set {x, y}` / `!!omap` | `{x:null,y:null}` / array | same | same shapes |
| `!foo bar` | error `unknown tag !<!foo> at line 1, column 12:...` | error `unknown tag !<!foo> (1:12)...` | `TaggedValue`; serde_json conversion error `invalid type: enum` |
| `!!js/regexp /x/` | error unknown tag | error unknown tag | **silently string "/x/"** |
| tab-indented child (`a:\n\tb: 1`) | **`{"a":null,"b":1}`** (accepted!) | error `tab characters must not be used in indentation` | error `found character that cannot start any token ...` |
| `''` (empty file) | `undefined` | `undefined` | `Null` |
| comment only | `null` | `null` | `Null` |
| plain scalar document | string | string | string |
| quoting, `|` and `>` blocks, `\xe9`, `''` escapes | identical | identical | identical |

## Error messages (user-visible)
mergeable writes `Invalid YML format > ${error.message}` into a check for `YAMLException` (`configuration.js:201-202`,
`settings.js:143-144`). v3 message = `reason + ' ' + mark.toString()` (`lib/js-yaml/exception.js:40`), mark =
`at line L, column C:\n<snippet>` with 1-based line/column and a 4-space-indented snippet plus caret (`mark.js:55-73`).
Probes:
- unclosed flow: v3 `unexpected end of the stream within a flow collection at line 2, column 1:\n    \n    ^`;
  Rust `did not find expected node content at line 2 column 1, while parsing a flow node`
- bad indentation: v3 `bad indentation of a mapping entry at line 3, column 2:\n     c: 2\n     ^`;
  Rust `did not find expected key at line 3 column 2, while parsing a block mapping`
- `title: foo: bar`: v3 `incomplete explicit mapping pair; a key node is missed; or followed by a non-tabulated empty line at line 1, column 11:\n    title: foo: bar\n              ^`;
  Rust `mapping values are not allowed in this context at line 1 column 11`
Reasons never match (libyaml vs js-yaml parser). Line/column usually agree (both 1-based). Recommendation: treat YAML error
text as a known diff (ADR-005), but keep the prefix `Invalid YML format > ` and the error *classification* (YAML error vs API
error) identical. Via probot `context.config`, a YAML error is **not** a YAMLException (see probot.md section 9).

## How to get Rust to match
1. Parse with an event/AST API that exposes the scalar style (plain vs quoted), because js-yaml resolves implicit types only for
   plain scalars and serde_yaml_ng's `Value` erases that distinction. Pure-Rust option: `yaml-rust2` 0.13
   (`parser::Event::Scalar(String, TScalarStyle, usize, Option<Tag>)`, `TScalarStyle::{Plain, SingleQuoted, DoubleQuoted,
   Literal, Folded}`, `src/parser.rs:61`, `src/scanner.rs:30`; https://docs.rs/yaml-rust2). serde_yaml_ng depends on
   `unsafe-libyaml` (`serde_yaml_ng-0.10.0/Cargo.toml:52`), i.e. transpiled C with `unsafe`.
2. Implement two resolvers: `js_yaml_3_safe` (YAML 1.1 ints incl. `_`, base 60, leading-0 octal, `0o` as string; floats with
   `_` and base 60; timestamps) and `js_yaml_4_default` (YAML 1.2 core: `0o` octal, no base 60, `08` -> 8). Use the v3 one
   for the three `safeLoad` sites and v4 for the `context.config` path.
3. Represent timestamps: mergeable never computes with them; they matter only when rendered or JSON-serialized. Keep a
   string in ISO form `YYYY-MM-DDTHH:MM:SS.mmmZ` (what `JSON.stringify` produces) and document the handlebars `String(Date)`
   difference as a known diff (it depends on the server TZ anyway).
4. Merge keys: implement js-yaml semantics: at the `<<` position insert keys of the referenced mapping(s) that are not already
   present; later explicit keys override (`loader.js:268-335`, `mergeMappings` / `storeMappingPair` with `overridableKeys`).
5. Duplicate keys and multi-document: error (both libraries already do).
6. Numbers: store integers > 2^53 as f64 to mimic JS (`12345678901234567890` -> `12345678901234567000`), and render f64 with
   JS Number formatting (`1.0` -> `1`). Non-finite values cannot live in `serde_json::Value`: they become `null` -- mergeable has
   no numeric settings where `.inf` makes sense; accept as known diff.
7. Empty document -> treat like JS `undefined` (settings: falls to defaults because `!settings.mergeable`; configuration: PR
   branch path returns undefined -> `new Configuration(undefined)`).
8. Tags: unknown tags must be an error (JS: `unknown tag`), `!!binary` -> mimic Buffer JSON or accept known diff.
9. Tabs: js-yaml 3 accepts tab-indented children as siblings (`{"a":null,"b":1}`); niche, document as known diff.

## Traps for the port
- [ ] Two loaders: v3 `safeLoad` for settings/PR config/org config, v4 `load` for `context.config` (normal config path).
- [ ] serde_yaml_ng does not apply `<<` merges unless `apply_merge()` is called, and then appends keys at the end (JS inserts in place).
- [ ] Unquoted dates become JS Dates (JSON -> ISO with `.000Z`, template -> local time string); serde_yaml_ng keeps the raw string.
- [ ] v3: `017` = 15, `0o17` = "0o17", `1_000` = 1000, `190:20:30` = 685230; v4: `0o17` = 15, `017` = 17, `08` = 8; serde_yaml_ng: `017`, `1_000`, `190:20:30` stay strings.
- [ ] yes/no/on/off are strings in all three (no YAML 1.1 booleans).
- [ ] Float `1.0` renders `1` in JS, `1.0` from serde; `.inf`/`.nan` become `null` in serde_json.
- [ ] Big integers lose precision in JS; Rust keeps them exact.
- [ ] Duplicate keys / multi-doc / syntax errors: same outcome (error), different message text -> known diff; keep `Invalid YML format > ` prefix and the YAML-vs-API classification.
- [ ] Unknown tags: JS errors, serde_yaml_ng returns `TaggedValue` (and `!!js/regexp` as a plain string) -> reject explicitly.
- [ ] Empty file: JS `undefined` (safeLoad) / `{}` (probot config: `load(data) || {}`), serde: `Null`.
- [ ] Complex (sequence) keys become `"a,b"` in JS, an error when converting serde_yaml_ng to serde_json.
