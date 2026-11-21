# handlebars.js 4.7.7 + lodash 4.17.21 `_.merge` vs Rust `handlebars` 6.4.4

Scope: every template mergeable renders: `lib/actions/handlebars/populateTemplate.js` (checks title/summary/text, comment body)
and `lib/actions/merge.js:15-20` (commit_title / commit_message rendered against `pullRequest.data`, helpers registered
globally by populateTemplate are visible there too because both use the same `handlebars` singleton).
Evidence: installed sources under `node_modules/handlebars/lib/handlebars/` (4.7.7), crate source
`~/.cargo/registry/src/*/handlebars-6.4.4/`, plus probe runs of the same templates on both engines
(Node 22 via devenv; Rust probe crate in the session scratchpad). Outputs below are copied from those runs.

## 1. Escaping

| | handlebars.js | Rust handlebars 6.4.4 |
|---|---|---|
| `{{x}}` escaped set | `& < > " ' \` =` -> `&amp; &lt; &gt; &quot; &#x27; &#x60; &#x3D;` (`utils.js:1-9` of lib, `dist/cjs/handlebars/utils.js:11-22`) | identical set and entities, `support.rs:41-56` (`str::escape_html`, comment links handlebars.js utils.js) |
| `{{{x}}}` / `{{&x}}` | no escaping | no escaping |
| `\\{{x}}` | literal `{{x}}` | literal `{{x}}` |
| non-ASCII | untouched | untouched |

Probe: `{{x}}` with `&<>"'\`= /\` -> `&amp;&lt;&gt;&quot;&#x27;&#x60;&#x3D; /\` on both. **Default escape fn already matches; do not
call `register_escape_fn`.** (Older crate versions only escaped `&"<>`; the doc comment on `registry.rs:48` still says so but the
code at `support.rs:41` escapes all 7.)

Helper results:
- JS: the return value of a helper in `{{helper}}` goes through `escapeExpression` (`lib/handlebars/utils.js:70-91`) unless it is a
  `SafeString` (`toHTML`, `utils.js:73`, `safe-string.js`). Probe: `{{displaySettings s}}` -> `&#x60;&#x60;&#x60;{&quot;a&quot;...`;
  mergeable's defaults use `{{{displaySettings ...}}}` and `{{{statusIcon status}}}` (`lib/configuration/lib/consts.js:27-30`) so they are raw.
- Rust: helpers implemented via `HelperDef::call_inner` (returning a `ScopedJson`, e.g. `handlebars_helper!`) are escaped by the
  default `call` (`helpers/mod.rs:123-131` -> `do_escape`, `render.rs:829-833`), and `{{{h}}}` sets `disable_escape`
  (`render.rs:880`). Helpers implemented as `call` that write to `Output` themselves are **never escaped** unless they call the
  escape fn.
- `SafeString` equivalent (`breaklines`, `populateTemplate.js:5-9`): implement `breaklines` with `call`, escape the input with
  `handlebars::html_escape`, replace line breaks with `<br>`, write to `out` directly (no second escape). For all other helpers
  (`toUpperCase`, `formatDate`, `displaySettings`, `statusIcon`) use `call_inner` returning `ScopedJson::Derived(Value::String)`
  so `{{h}}` escapes and `{{{h}}}` does not, as in JS.
- `breaklines` JS regex `/(\r\n|\n|\r|\n\n)/gm`: `\r\n` is one break, `\n\n` is two `<br>` (alternation order), `\r` alone is one.

## 2. Rendering of values (`{{x}}` / `{{{x}}}`)

JS stringifies with `'' + value` (`utils.js:75-84`); `null`/`undefined` -> `''`, `false` -> `'false'`, `0` -> `'0'`.
Rust: `JsonRender for Value` (`json/value.rs:118-140`).

| value | JS output | Rust default | Fix |
|---|---|---|---|
| `["a","b",{"c":1},null,[1,2]]` | `a,b,[object Object],,1,2` | `[a, b, [object], , [1, 2]]` | own stringify |
| `{"a":1}` | `[object Object]` | `[object]` | own stringify |
| `[]` / `[[]]` | `` / `` | `[]` / `[[]]` | own stringify |
| `true` / `false` / `null` / `0` | `true`/`false`/``/`0` | same | - |
| `1.0` (serde f64) | `1` | `1.0` | JS Number::toString |
| `-0.0` | `0` | `-0.0` | JS Number::toString |
| `1.2345678901234568e20` | `123456789012345680000` | `1.2345678901234568e+20` | JS Number::toString |
| `1e21` / `1e-7` | `1e+21` / `1e-7` | `1e+21` / n/a | JS Number::toString |
| missing path | `` | `` | - |

Rust handlebars has no hook to replace `JsonRender` for plain `{{path}}` (`render.rs:904-906` calls `context_json.value().render()`
directly). Options: (a) pre-process the data so every array/object leaf that a template could print is already a JS-stringified
string -- impossible in general because the same value is also iterated by `#each`; (b) rewrite every `{{path}}` /
`{{{path}}}` mustache without helper into `{{js path}}` / `{{{js path}}}` before compiling, where `js` is a `call_inner` helper
that returns `String(value)` from the `js` module (ADR-006). (b) is the faithful route. Numbers: serde_json keeps integers as
`u64/i64` (render `1`) only when the JSON text had no fraction; YAML `1.0` or arithmetic results become `f64` -> implement JS
`Number.prototype.toString` (shortest round-trip, exponent when `>= 1e21` or `< 1e-6`; `ryu-js` crate implements exactly this).

## 3. Lookup, missing fields, paths

- Missing field renders empty in both (`helper-missing.js:13-16` returns `undefined` for a bare id; Rust `render.rs:893-901`
  writes nothing when no `helperMissing` is registered). `{{a.b}}` with `a: null` -> `` on both.
- `{{a.0}}` / `{{arr.1}}`: **JS parse error** (`Expecting 'ID', got 'NUMBER'`); Rust accepts and indexes. Only `{{a.[0]}}`
  works in JS. Harmless superset, but a template that errors in JS renders in Rust (error path differs).
- `{{arr.length}}` / `{{s.length}}`: JS `2` / `3` (property of the JS object). Rust: **RenderError**
  `InvalidJsonIndex("length")` for arrays and empty for strings. Fix: resolve `length` in the `js` helper of 2(b).
- `{{a/b}}`, `{{this.a}}`, `{{./a}}`, `{{../x}}`, `{{@root.x}}`, `{{[a b]}}`, `{{a-b}}`, `{{a.[c-d]}}`: same on both.
- Prototype properties (`{{constructor}}`, `{{toString}}`, `{{a.constructor}}`): JS returns `''` and logs
  "Access has been denied..." (`runtime.js:129-142`, `internal/proto-access.js:35-63`); Rust has no prototype: `''`. Same output.
- Inside `#each`, an unknown name does **not** fall through to the parent scope in either engine (`{{#each xs}}{{title}}` with
  `title` only on root -> `;` on both).

## 4. Block helpers

### `ifEquals` (`populateTemplate.js:34-36`)
`(arg1 === arg2) ? options.fn(this) : options.inverse(this)`. Rust: `HelperDef::call` rendering `h.template()` or `h.inverse()`
with the *current* context (`t.render(r, ctx, rc, out)`), `None` -> write nothing. Equality: JS `===`; serde_json
`Value == Value` differs for numbers: probe `ifEquals a 1` with `a: 1.0` -> JS `Y`, naive Rust `N`
(`Number(1.0) != Number(1)`). Compare numbers via `as_f64()`; strings/bools by value; arrays/objects are reference-equal in JS
-> always `false` unless the same reference (two path params pointing to the same object are `===`; literal templates cannot
produce that, treat array/object == as false). Missing param: JS `undefined === undefined` is `true`; Rust param is
`ScopedJson::Missing` (`json/value.rs:18`) -> check `is_value_missing()` on both sides; `null === undefined` is `false`.

### Other helpers receiving missing args
- `toUpperCase`: JS throws `TypeError: Cannot read properties of undefined (reading 'toUpperCase')` for undefined and
  `s.toUpperCase is not a function` for numbers. The whole populateTemplate throws. Rust must return a `RenderError` for
  non-strings (the caller maps it to the same check/error flow). Unicode: JS `'straße'.toUpperCase()` = `STRASSE`, Rust
  `str::to_uppercase` = `STRASSE` (same full case mapping).
- `displaySettings`: `JSON.stringify(undefined)` -> template literal gives `\`\`\`undefined\`\`\`` (probe). Rust: missing ->
  literal `undefined`; otherwise `serde_json::to_string` (compact, same key order with preserve_order; JS escapes U+2028/9
  not, serde_json neither; both escape `"` and control chars). Numbers differ: `JSON.stringify` writes `1.0` as `1` and `-0` as `0`,
  `serde_json::to_string` writes f64 `1.0` as `1.0` and `-0.0` as `-0.0` -> serialize numbers through the same JS Number formatter.
- `statusIcon` with non-string: `Unknown Status given: 5` -> use JS String(x) on the param.

### `#if` / `#unless` truthiness
JS `if.js:16`: inverse when `!value` or `isEmpty(value)` (`utils.js:93-101`: falsy or empty array). **Empty object `{}` is
truthy in JS** but falsy in Rust (`json/value.rs:154-174`, `Json::Object(ref i) => !i.is_empty()`). Probe `{{#if o}}` with
`o: {}`: JS `T`, Rust `F`. Fix: register own `if`/`unless` helpers using `js::truthy` + empty-array rule (override
built-ins by registering the same name).

### `#each` (`helpers/each.js` lines 38-41, 60-96 vs crate `helpers/helper_each.rs:84-150`)
| | JS | Rust |
|---|---|---|
| array: `@index` `@first` `@last` | yes | yes |
| array: `@key` | **= index** (`each.js:38`) | not set (renders ``). Probe `0truefalse0=a` vs `0truefalse=a` |
| object key order | `Object.keys`: integer-like keys ascending first, then insertion | serde_json map order: insertion with `preserve_order`, sorted without |
| object `{z:1,a:2,"1":3}` | `1,z,a` | preserve_order: `z,a,1`; default: `1,a,z` |
| empty array/object | `{{else}}` | `{{else}}` |
| string | inverse (strings are not iterated, `each.js:57`) | inverse |
| sparse array hole | skipped (`i in context`) | n/a in JSON |
Fix for `@key` + key order: override `each` with a port of `each.js` (set `@key`, order object keys JS-style).

### Mustache sections and inverted sections (**parse failure in Rust**)
mergeable's own default check title uses one: `{{#each failures}} {{toUpperCase name}}{{^@last}}, {{/@last}}{{/each}}`
(`lib/configuration/lib/consts.js:18`). JS renders `a, b, c`. Rust 6.4.4: **template error** `expected identifier or
trailing_tilde_to_omit_whitespace` -- the grammar only knows `{{^}}` as `else` inside a block (`grammar.pest:79-83`).
JS handles `{{^x}}..{{/x}}` as an inverse block of `blockHelperMissing` and `{{#o}}..{{/o}}` on non-helpers via
`blockHelperMissing` (`helpers/block-helper-missing.js`: true -> fn(this), false/null -> inverse, array -> each, object -> fn(object)).
Rust `{{#o}}`: `HelperNotFound("o")` unless a `blockHelperMissing` helper is registered (`render.rs:689-694, 815-820`).
Fix: (1) source rewrite before compile: `{{^X}}` (X not `else`, at block start) -> `{{#unless X}}` and its matching `{{/X}}` ->
`{{/unless}}` (track nesting); (2) register `blockHelperMissing` implementing `block-helper-missing.js`.

### Whitespace / standalone lines
Probes `a  {{~x~}}  b`, standalone `{{#if}}`/`{{#each}}` lines, CRLF templates: identical output on both
(`"aXb\n  line\nend"`, `"- 1\n- 2\n"`, `"a\r\nb\r\nc"`). Comments `{{! }}` / `{{!-- --}}` identical. Block params
`as |item i|`, `#with`, `lookup` identical.

## 5. Strict mode, errors
- mergeable never passes `strict`; Rust default `strict_mode = false` (`registry.rs:222`). Keep it off.
- Missing helper with args: JS `Missing helper: "nohelper"` (`helper-missing.js:19`); Rust `HelperNotFound` ->
  `Helper not found nohelper`. Parse errors differ completely (JS `Parse error on line 1:\n{{x\n--^\nExpecting 'ID', ...`,
  `each doesn't match if - 1:3`; Rust `Failed to parse template Template error: ...`). JS compiles lazily: `handlebars.compile`
  only parses on the first call, so both parse and render errors surface at `handlebarsTemplate(...)` in populateTemplate.
  If any case puts `error.message` into GitHub output, it is a known diff (ADR-005).

## 6. lodash `_.merge({}, payload, validationResult)` (`populateTemplate.js:56`)
Source: `node_modules/lodash/lodash.js:3638-3657` (baseMerge), `3675-3733` (baseMergeDeep), `2496-2501` (assignMergeValue).
Probe results (`_.merge({}, A, B)`):
- deep object merge: `{o:{x:1,y:{z:1}}}` + `{o:{y:{w:2}}}` -> `{o:{x:1,y:{z:1,w:2}}}`.
- **arrays merged by index, not replaced**: `{a:[1,2,3]}` + `{a:[9]}` -> `{a:[9,2,3]}`; `{a:[{x:1,y:1}]}` + `{a:[{x:2}]}` ->
  `{a:[{x:2,y:1}]}` (lines 3696-3700: existing array reused, then recursive merge).
- `undefined` source values are skipped when the key exists (`assignMergeValue`: `value === undefined && key in object` -> no
  write); `{a:[1,2]}` + `{a:[undefined,3]}` -> `[1,3]`. **`null` overwrites** (`{b:2}` + `{b:null}` -> `{b:null}`).
  serde_json has no undefined, so: every `Value` (incl. `Null`) overwrites.
- primitive over object: replaced (`{a:{b:2}}` + `{a:1}` -> `{a:1}`); object over primitive: deep-cloned object.
- array source over object target: target replaced by a new array (line 3710 `newValue = []` then merged) -> `{a:[5]}`.
- object source over array target: target array kept and the object's keys set on it as properties (line 3716 `newValue = objValue`);
  non-index keys vanish in JSON, numeric-string keys (`"0"`) overwrite elements. Edge case; port as: keep array, apply keys that
  parse as indices.
- key order: target keys first, new keys appended (`{b,a}` + `{c,a}` -> `{b,a,c}`) -- matches `serde_json::Map` with
  `preserve_order` when updating in place (`insert` on an existing key keeps its position in indexmap).
- string over array: replaced (`{a:[1]}` + `{a:'x'}` -> `{a:'x'}`).
- The merge deep-clones (`initCloneObject`), so templates never mutate the payload; in Rust clone into a fresh `Value`.

Rust sketch:
```rust
fn lodash_merge(target: &mut Value, source: &Value) {
    match (target, source) {
        (Value::Object(target_map), Value::Object(source_map)) => {
            for (key, source_value) in source_map {
                match target_map.get_mut(key) {
                    Some(existing) if source_value.is_object() || source_value.is_array() => lodash_merge(existing, source_value),
                    _ => { target_map.insert(key.clone(), deep_merge_into_empty(source_value)); }
                }
            }
        }
        (Value::Array(target_items), Value::Array(source_items)) => { /* merge index-wise, extend when longer */ }
        (target_slot, source_value) => *target_slot = deep_merge_into_empty(source_value),
    }
}
```
(`Some(existing)` with an object source and a primitive `existing` must *replace*, and array source over object must replace;
encode the 3696-3725 table exactly.)

## Traps for the port
- [ ] Keep the crate's default escape fn (already the JS 7-char set); never `no_escape`.
- [ ] `breaklines` writes pre-escaped text via `call` (SafeString); other helpers via `call_inner` so `{{h}}` escapes and `{{{h}}}` does not.
- [ ] Arrays render `a,b`, objects `[object Object]`, `[]` renders ``: rewrite plain mustaches to a `js` stringify helper.
- [ ] Floats render JS-style (`1` not `1.0`, `-0` -> `0`, `1e+21`): use JS Number::toString (ryu-js).
- [ ] `{{x.length}}` on arrays/strings must work (Rust errors on arrays).
- [ ] `{{^@last}}...{{/@last}}` in the default check title does not parse in Rust: rewrite inverted sections to `#unless`.
- [ ] Register `blockHelperMissing` so `{{#obj}}`/`{{#bool}}`/`{{#array}}` sections behave like JS.
- [ ] Override `if`/`unless`: `{}` is truthy in JS, falsy in Rust.
- [ ] Override `each`: `@key` = index for arrays; object keys in JS order (integer-like keys first ascending).
- [ ] `ifEquals`: numeric equality via f64 (1 vs 1.0), missing === missing is true, missing !== null, objects never equal.
- [ ] `toUpperCase` on non-string must fail the render (JS throws TypeError), not render empty.
- [ ] `displaySettings` of a missing value prints `undefined`.
- [ ] Strict mode off; missing paths render empty.
- [ ] Parse/render error texts differ between engines: treat as known diff if they reach GitHub.
- [ ] `_.merge`: arrays merge by index, `null` overwrites, primitive replaces object, array replaces object, target key order kept.
- [ ] Helpers are global in JS: merge.js commit_title/commit_message templates can use `ifEquals` etc. -> one shared registry.
