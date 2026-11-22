const fs = require('fs')
const path = require('path')
process.env.TZ = 'UTC'
const ORIG = process.argv[2]
const OUT = process.argv[3]
const jsYaml3Directory = path.join(ORIG, 'node_modules/js-yaml')
const jsYaml4Directory = path.join(ORIG, 'node_modules/@probot/octokit-plugin-config/node_modules/js-yaml')
if (!require(path.join(jsYaml3Directory, 'package.json')).version.startsWith('3.')) throw new Error('expected js-yaml 3 at node_modules/js-yaml')
if (!require(path.join(jsYaml4Directory, 'package.json')).version.startsWith('4.')) throw new Error('expected js-yaml 4 under @probot/octokit-plugin-config')
const jsYaml3 = require(jsYaml3Directory)
const jsYaml4 = require(jsYaml4Directory)

const inputs = []
const add = (...texts) => texts.forEach(text => inputs.push(text))

function caseConfigs () {
  const casesDir = path.join(ORIG, 'rust/app-cases/cases')
  const configs = []
  for (const name of fs.readdirSync(casesDir).sort()) {
    if (!name.endsWith('.json')) continue
    const caseFile = JSON.parse(fs.readFileSync(path.join(casesDir, name), 'utf8'))
    for (const mock of caseFile.github || []) {
      const body = mock.body
      if (body && typeof body.content === 'string' && /\.ya?ml$/.test(body.path || mock.path || '')) {
        configs.push(Buffer.from(body.content, body.encoding === 'base64' ? 'base64' : 'utf8').toString('utf8'))
      }
    }
  }
  return configs
}

function docBlocks () {
  const blocks = []
  const walk = directory => {
    for (const name of fs.readdirSync(directory).sort()) {
      const full = path.join(directory, name)
      if (fs.statSync(full).isDirectory()) walk(full)
      else if (name.endsWith('.rst')) {
        const lines = fs.readFileSync(full, 'utf8').split('\n')
        for (let index = 0; index < lines.length; index++) {
          if (!/::\s*(yml|yaml)?\s*$/.test(lines[index])) continue
          let cursor = index + 1
          while (cursor < lines.length && lines[cursor].trim() === '') cursor++
          const block = []
          while (cursor < lines.length && (lines[cursor].trim() === '' || /^\s/.test(lines[cursor]))) block.push(lines[cursor++])
          const indents = block.filter(line => line.trim()).map(line => line.match(/^\s*/)[0].length)
          if (!indents.length) continue
          const indent = Math.min(...indents)
          blocks.push(block.map(line => line.slice(indent)).join('\n').replace(/\s+$/, '') + '\n')
        }
      }
    }
  }
  walk(path.join(ORIG, 'docs'))
  return blocks
}

const configs = [...new Set(caseConfigs())]
const docs = [...new Set(docBlocks())]
add(...configs, ...docs)

const scalars = [
  '~', 'null', 'Null', 'NULL', 'nULL', '', 'true', 'True', 'TRUE', 'false', 'False', 'FALSE', 'yes', 'no', 'on', 'off', 'y', 'n', 'tRUE',
  '0', '-0', '+0', '00', '012', '0o12', '0o', '0o8', '0b1010', '-0b11', '0b', '0b_1', '0b1_', '0b12', '0x1F', '0xff_ff', '0X1F', '0x', '-0x1A', '0xG', '09', '019', '0_7', '1_000', '1__0', '_1', '1_', '685230', '+685230', '-685230',
  '190:20:30', '-1:30', '1:60', '1:5', '1:30:', '1_0:30', '12345678901234567890', '9007199254740993', '0x1FFFFFFFFFFFFF1', '0b' + '1'.repeat(70), '0777777777777777777777', '0x' + 'f'.repeat(300), '1' + '0'.repeat(400),
  '1.0', '-1.5', '+1.5', '.5', '-.5', '+.5', '1.', '1.e5', '1e5', '1E5', '1e+5', '1e-5', '-1e-5', '6.8523015e+5', '685.230_15e+03', '685_230.15', '190:20:30.15', '-190:20:30.15', '1:30.5', '1:30.', '0:0.5',
  '.inf', '-.inf', '+.inf', '.Inf', '.INF', '.NaN', '.nan', '.NAN', '-.nan', 'inf', 'nan', '0.0', '-0.0', '01.5', '00.5', '1_.5', '._5', '._e5', '._', '1.5_', '1e', '1e5.5', '3.14159265358979323846', '1e400', '-1e400', '5e-324', '1e-400', '0.1', '100000000000000000000000', '123456789.123456789', '1e21', '1e-7', '0.000001', '1.5e300',
  '2001-12-14', '2001-12-14t21:59:43.10-05:00', '2001-12-14T21:59:43.10-05:00', '2001-12-14 21:59:43.10 -5', '2001-12-14 21:59:43.10', '2001-12-15T02:59:43.1Z', '2001-1-1', '2001-01-01 1:2:3', '2001-1-1 01:02:03', '2001-02-30', '2001-13-01', '2001-1-40 10:00:00', '0050-01-01', '0099-12-31', '0100-01-01', '9999-12-31T23:59:59-23:59', '2001-12-14T21:59:43.123456Z', '2001-12-14T21:59:43.Z', '2001-12-14 21:59:43 +01:30', '2001-12-14T21:59:43   Z', '2001-12-14T21:59:43+1', '2001-12-14\t21:59:43', '2001-12-14T21:59', '2001-00-00', '2001-12-14T24:00:00Z', '2001-12-14T21:59:60Z', '2001-12-14T21:59:43-00:30', '20011214', '2001-12-14T21:59:43.1+05',
  '<<', '<', 'hello', 'hello world', 'a#b', 'a #b', 'http://example.com:80/x', '-dash', '?q', ':c', '%pct', '"double"', "'single'", '"esc \\t \\n \\u00e9 \\x41 \\U0001F600 \\N \\_ \\L \\P \\0 \\a \\b \\e \\v \\f \\r \\/ \\\\ \\" \\ "', "'it''s'", '"a\\qb"', '"\\x4"', '"\\xZZ"', '"\\u12"', '"\\U0010FFFF"', '"\\U00110000"', '"\\UFFFFFFFF"', '"\\x00"', '"\\x85"', '"\\x07"', '"\\u00"', '"\\uFFFF"', '"\\uFEFF"',
  '@at', '`back', '&anchor value', '*alias', '!tag value', '|', '>', 'multi word plain', 'trailing space   ', 'a:b', 'a: b', 'a,b', 'a[b]', 'a{b}', '-', '?', ':', '- x', '? x', 'é', '😀', 'ü:ß', '"😀"', "'é'",
]
const scalarTemplates = [s => 'key: ' + s, s => '- ' + s, s => '[' + s + ']', s => '{a: ' + s + '}', s => s + ': value', s => s, s => '? ' + s + '\n: v', s => 'a:\n  b: ' + s + '\n  c: 1']
for (const scalar of scalars) for (const template of scalarTemplates) add(template(scalar))

add(
  '!!str 123', '!!int "12"', '!!int 0x1F', '!!int 1.5', '!!float 1', '!!float "1.5"', '!!float .inf', '!!bool yes', '!!bool "true"', '!!null ""', '!!null ~', '!!null x', '!!timestamp 2001-12-14', '!!timestamp x', '!!timestamp "2001-12-14 1:2:3"',
  '!!binary SGVsbG8=', '!!binary "SGVs\\nbG8="', '!!binary |\n  SGVsbG8gd29y\n  bGQ=\n', '!!binary "abc"', '!!binary ""', '!!binary "SGk"', '!!binary "S=G=k="', '!!binary "a b"', '!!binary "\\u00e9"', 'a: !!binary R0lGODlhDAAMAIQAAP//9/X17unp5WZmZgAAAOfn515eXvPz7Y6OjuDg4J+fn5OTk6enp56enmlpaWNjY6Ojo4SEhP/++f/++f/++f/++f/++f/++f/++f/++f/++f/++f/++f/++f/++f/++SH+Dk1hZGUgd2l0aCBHSU1QACwAAAAADAAMAAAFLCAgjoEwnuNAFOhpEMTRiggcz4BNJHrv/zCFcLiwMWYNG84BwwEeECcgggoBADs=',
  '!!seq [1]', '!!seq {a: 1}', '!!map {a: 1}', '!!map [1]', '!!omap [{a: 1}, {b: 2}]', '!!omap [{a: 1}, {a: 2}]', '!!omap [{a: 1, b: 2}]', '!!omap [1]', '!!omap\n- a: 1\n- b: 2\n', '!!omap []', '!!omap', '!!pairs [{a: 1}, {a: 2}]', '!!pairs [{a: 1, b: 2}]', '!!pairs\n- a: 1\n- a: 2\n', '!!pairs', '!!pairs [{}]', '!!set {a, b}', '!!set {a: 1}', '!!set\n? a\n? b\n', '!!set', '!!set {a: ~, b: null}',
  '!foo bar', '!<tag:yaml.org,2002:str> 12', '!<!> 12', '! 12', '!!python/object x', '!e!foo x', '%TAG !e! tag:example.com,2000:\n---\n!e!foo x', '%TAG !e! tag:yaml.org,2002:\n---\n!e!str 12', '%TAG !e! tag:yaml.org,2002:\n---\n!e!str 12\n---\n!e!str 13', '%TAG ! tag:yaml.org,2002:\n---\n!int "7"', '%TAG !! tag:x:\n---\n!!int 1',
  '!!str', '!!int', '!!seq', '!!map', '!!float', '!!bool', '!!null', '!!binary', '!!timestamp', '!!merge', '!!str\n1', '!!str\n  1', '!!int\n"12"', '!!float\n1', '!!float\n"1"', '!!binary\n12', '!!binary\ntrue', '!!binary\n!!binary SGk=', '!!int\n!!binary SGk=', '!!timestamp\n!!binary MjAyNC0wMS0wMQ==', '!!str !!int 1', '!!seq\n- a', '!!map\na: 1', '!!seq\na: 1', '!!str\na: 1', '!!str\n[1]', '!!seq\n[1]', 'a: !!seq\n  - 1\n',
  '!<?> foo', '!<?> 12', '!<?> [1]', '!<?>\n1', '!<?>\n"x"', '!!merge <<', '!e%21 x', '!a%zz x', '!%C3%A9 x', '!%FF x', '!%E9 x', '!<tag:%E9> x', '!<tag:%C3%A9> x', '!a!b!c x', '!a!b x', '!a[b] x', '!<abc x', '!! x', '!!<x>', '!<> x', '!<>x> y', '!a,b c', '!é x', '!😀 x', '!!😀 x', 'a: !!str', 'a: !!int', '- !!str\n- !!null', '[!!str , !!int 1]', '{!!str : !!str }', '!!str &a x\nb: *a', '&a !!str x',
  'a: &x 1\nb: *x', 'a: &x [1, 2]\nb: *x', 'a: &x {k: v}\nb: *x', 'b: *x', 'a: &x\n  k: v\nb: *x', 'a: &x\n  foo\nb: *x', 'a: &x\n  [1, 2]\nb: *x', 'a: &x\n  1\nb: *x', 'a: &x\n  - 1\nb: *x', 'a: & 1', 'a: *', 'a: * ', 'a: &x &y 1', 'a: &x *y', 'a: &x 1\nb: &y *x', 'a: &x 1\nb: !!str *x', 'a: &__proto__ 1\nb: *__proto__', 'a: &constructor 1\nb: *constructor', 'a: &hasOwnProperty 1\nb: *hasOwnProperty', 'a: &x 1\na2: &x 2\nb: *x', 'a: &x [1]\nb: *x\nc: *x', '- &a a\n- *a\n- &b [*a]\n- *b', '[&a 1, *a, &b {x: *a}, *b]', 'a: &x\nb: *x', '&x a: 1', '&x\na: 1', '? &k key\n: *k', '&a [1]: *a', 'a: &x 1\n---\nb: *x', 'x: &a, 1', 'x: [&a, *a]', 'x: [&a , *a]', 'x: &a\n- 1', '&x\n- 1\n- *x',
  'base: &b {a: 1, b: 2}\nderived:\n  <<: *b\n  b: 3', 'a: &a {x: 1}\nb: &b {y: 2}\nc:\n  <<: [*a, *b]\n  z: 3', 's: &s [1, 2]\nm:\n  <<: *s', 'm:\n  <<: 1', 'm:\n  <<: [1]', 'm:\n  <<: !!binary SGk=', 'm:\n  <<: 2001-01-01', 'm:\n  <<: "str"', 'm:\n  <<: ~', 'm:\n  <<: [~]', 'm:\n  <<: {a: 1}\n  a: 2', 'm:\n  a: 2\n  <<: {a: 1}', 'm:\n  <<: {a: 1}\n  <<: {a: 2}', 'm:\n  <<: {a: 1}\n  a: 2\n  a: 3', 'm:\n  ? <<\n  : {a: 1}', 'm:\n  !!merge <<: {a: 1}', 'm:\n  "<<": {a: 1}', '{<<: {a: 1}, b: 2}', '[<<: {a: 1}]', 'm:\n  <<: {1: a, b: c, 0: d}', 'm:\n  <<: [{a: 1}, {a: 2, b: 2}]', '<<: {a: 1}', '<<: [[1]]', 'a: &a\n  x: 1\nb:\n  <<: *a\n  <<: *a', 'a: {<<: &b {k: 1}, c: *b}', 'm:\n  <<: {__proto__: 1}', 'm:\n  <<: {"__proto__": {a: 1}}',
  'b: 1\na: 2\n2: x\n1: y\n-1: z\n01: w\n4294967295: big\n4294967294: max\n" 3": s\n1.5: f', '1: a', '1.5: a', '0x10: a', 'true: a', 'null: a', '~: a', '.inf: a', '-.inf: a', '.nan: a', '-0: a', '-0.0: a', '1e3: a', '1e21: a', '0.0000001: a', '2001-12-14: a', '2001-12-14 21:59:43.10 -5: a', '? [1, 2]\n: v', '[1, 2]: v', '[a, [b]]: v', '{a: 1}: v', '[{a: 1}, b]: v', '? !!binary SGk=\n: v', '[1, null, ~, true, 1.5]: v', '[2001-12-14]: v', '? \n: v', ': v', '"": v', "'': v", '[]: v', '{}: v', '__proto__: 1', '__proto__: {a: 1}', '__proto__: [1]\nb: 2', '__proto__: 1\n__proto__: 2', 'constructor: 1\ntoString: 2\nhasOwnProperty: 3', '{__proto__: {a: 1}, b: 2}', '!!set {__proto__}', '!!omap [{__proto__: 1}]', '!!pairs [{__proto__: 1}]',
  'a: 1\na: 2', 'a: 1\nb:\n  c: 1\na: 2', 'a:\n  b: 1\n  b: 2', '{a: 1, a: 2}', '[a: 1, a: 2]', '1: a\n1.0: b', '? a\n? a', 'a: 1\n"a": 2', 'a: 1\na:\n  b: 2\n  c: 3', 'x:\n  a: 1\n  a: |\n    text\n    more\n', 'a: 1\nb: 2\nc: 3\nb: 4', 'top:\n  - a: 1\n    b: 2\n    a: 3', '{a: 1,\n a: 2}', '{\n  a: 1,\n  a: 2\n}', '? a\n: 1\n? a\n: 2', 'a:\n  - 1\na: 2', 'x: 1\ny: 2\n? x\n', '[{a: 1, a: 2}]', 'a: 1\n? a', '? a\na: 1',
  'a: |\n  line1\n  line2\n', 'a: >\n  line1\n  line2\n', 'a: |-\n  x\n\n', 'a: |+\n  x\n\n', 'a: >-\n  x\n  y\n\n', 'a: >+\n  x\n\n\n', 'a: |2\n   x\n  y\n', 'a: |1-\n  x', 'a: |-1\n  x', 'a: |0\n  x', 'a: |++\n  x', 'a: |22\n  x', 'a: | # comment\n  x', 'a: |x\n  x', 'a: |\n\n  text\n\n\n', 'a: >\n  a\n  b\n\n  c\n   d\n  e\n\n  f\n', 'a: >\n\n\n  a\n', 'a: |\n  a\n   b\n  c\n', '|\nfoo\nbar\n', '>\n foo\n bar\n', '--- |\n  text\n', '--- >\n  folded\n  text\n...\n', 'a: |\n  \ttab\n', 'a: >\n  \tindented tab\n  next\n', 'a: |\n  x\nb: 1', 'a: |\n x\n  y\n z', 'a: |\n  x\n y', 'a: |', 'a: >', '- |\n  a\n- >\n  b\n', 'a: |\n    deeper\n  less\n', 'a: |-', 'a: |\n  trailing  \n', 'a: |+\n', 'a: >-\n  a\n\n\n  b\n', 'a: |\n  é😀\n', 'a: |3\n  x\n',
  '[1, 2, 3]', '[1, 2, ]', '[1,, 2]', '[ , ]', '[,]', '{,}', '{a: 1, b}', '{a, b: 2}', '{a: 1 b: 2}', '[1 2]', '[1, [2, [3]]]', '{a: [1, {b: 2}]}', '[a: 1]', '[? a : b]', '{? a}', '[? a]', '[1, 2', '{a: 1', '[1, 2]]', ']', '}', '{a: 1}}', '[\n  1,\n  2\n]', 'key: [\n  1,\n 2]', 'key: [\n1]', '{"a":1,"b":[true,false,null]}', '{"a":1}', '["a":1]', '{a:1}', '[a:1]', '{a: b: c}', '[a, b]: c', '{a: [b, c}', '[a, {b: c]', '{"a"  :  1}', '["a"\n: 1]', '[a\n: 1]', '{a\n: 1}', '[1, # comment\n 2]', '[1 # comment\n, 2]', '{a: 1, #c\n b: 2}', '[a, b, c] trailing', '{a: 1} trailing', '[-1, - 1, -a, - a]', '[:a, a:, a:b]', '[http://x.y]', '{http://x.y: 1}', '[a,b,c]', '{a,b,c}', '{a: 1, b: 2,}', '[[], {}, [[]], {{}: 1}]', '[a, b]\n[c]', "['a', \"b\"]", '[a, \'b\' c]', '{a: \'b\' c}', 'a: [1, 2]\n  b: 3', 'a: {x: 1}\nb: [', '- [a,\n  b]\n- c',
  '# only comment', 'a: 1 # c', 'a: 1# not comment', 'a: "x" # c', '- # c\n  1', '#c\n---\na: 1\n...\n# trailing', 'a: 1\n# c\nb: 2', 'a: # c\n  b: 1', 'a: x # c\n  y', '#', '# a\n# b', 'a: "x"# c', "a: 'x'#c",
  '---\na: 1', '--- a', 'a: 1\n---\nb: 2', '---\n---', 'a: 1\n...\n', 'a: 1\n...\nb: 2', '...', '...\n', '---\n...\n---\n...', '%YAML 1.2\n---\na: 1', '%YAML 1.1\n%YAML 1.2\n---', '%YAML 2.0\n---', '%YAML 1.3\n---\na', '%YAML\n---', '%YAML 1.x\n---', '%YAML 1.2 extra\n---', '%TAG ! tag:x,2000:\n---\n!foo x', '%TAG !e! a b\n---', '%TAG e! x\n---', '%TAG !e! x\n%TAG !e! y\n---', '%TAG !e! ,x\n---', '%TAG !e!\n---', '%FOO bar\n---\na: 1', '%YAML 1.2\na: 1', '% x\n---', '%YAML 1.2 # comment\n---\n1', 'a: 1\n--- b', '---a', '--- # c\na: 1', '---\n', '', ' ', '\n', '   \n  \n', '\n\n\n', '--- !!str\n', '--- &a\n', 'a\n---', '---\na\n...\n...\n', '--- [1]\n--- {a: 1}', 'a: 1\n--- \n', '-- -', '--- -', '... a', 'a: |\n  x\n---\nb', ' a: 1', '  - 1\n  - 2', ' a: 1\n b: 2', ' a: 1\nb: 2',
  'a:\tb', 'a:\t1', '\ta: 1', 'a:\n\tb: 1', 'a:\n  b: 1\n\tc: 2', '- \t1', '-\t1', 'key:\t\tvalue', 'a: b\t# c', 'a: 1\t', '\t', 'a: [1,\t2]', 'a:\n  - 1\n\t- 2', '? \ta\n: b', '"a\tb": c', "a: 'x\ty'", '\t\n', 'a:\n \t- x', 'a:\n\t\n  b: 1', '- a\n\t- b', 'a: 1\n\tb: 2', '-\ta: 1', '- \ta: 1\n  b: 2', 'a:\n  b:\t1\n', '{a:\t1}', 'a: "\t"', 'a\t: 1', 'a\t:\t1',
  'a: 1\n b: 2', 'a:\n  b: 1\n c: 2', '- a\n - b', '- a\n  - b', 'a:\n- b', 'a:\n  - b\n - c', 'key: value\n  extra', 'a: b: c', 'a:b', '- a: 1\n  b: 2\n - c', 'version: 2\nmergeable:\n  - when: pull_request.*\n     validate: [\n', 'version: 2\nmergeable:\n  - when: pull_request.*\n    validate:\n      - do: title\n        must_exclude:\n          regex: wip\n       message: bad\n', 'a:\n    b: 1\n  c: 2', 'a:\n  b: 1\n   c: 2', '- - a\n   - b', '-a', '- ', '-', '- -', '- - -', '-\n-\n', '- a\n-b', 'a:\n  -b', '? a\n  : b', 'a: b\n  : c',
  '"unterminated', "'unterminated", '"a\n---\nb"', "'a\n...\n'", '"a\n  b\n\n  c"', "'a\n  b\n\n  c'", '"a \\\n  b"', '"a\\\n\n  b"', '"a: \x01"', 'a: "\x01"', "a: '\x01'", 'a: \x01', 'a: \x7f', 'a: x\u{FFFE}', 'a: \x85x', 'a: x\u{2028}y', 'a: x\u{2029}y', 'a\x85b: 1', '\u{FEFF}a: 1', 'a: \u{FEFF}b', 'a: \x00', '\x00', 'a: 1\n\x00', 'a: "x\xa0y"', '"\\u0041\\u00e9\\ud83d\\ude00"', "'a''b'''", "''", '""', '"\\""', "'\\'", '"multi\nline"', "'multi\n\nline'",
  'a: 1\r\nb: 2\r\n', 'a: |\r\n  x\r\n  y\r\n', 'a: 1\rb: 2', 'a: 1\r\n b: 2', 'a: >\r\n  x\r\n  y\r\n\r\n  z\r\n', 'a: "x\r\n  y"', '- a\r- b\r', 'a: 1\r\n\r\nb: [\r\n', 'a: 1\n\rb: 2',
  'key: ' + 'x'.repeat(100) + ': bad', 'k: v\n' + 'x'.repeat(30) + ': ' + 'y'.repeat(60) + ': bad\nnext: 1', 'short: [' + 'z'.repeat(200), 'a: 1\n' + ' '.repeat(50) + 'b: 2', 'x'.repeat(40) + ': 1\n x: 2', '"' + 'q'.repeat(90) + '\\q"', 'long: ' + 'w '.repeat(60) + '\n  bad: indent',
  'ä: 1\n ö: 2', '😀: 1\n 😀: 2', 'a: "😀\\q"', 'é😀é: [😀, é\n', '😀😀😀: 1\n😀😀😀: 2', 'k: ' + '😀'.repeat(30) + ': x',
  'a: foo\n  bar\n\n  baz', 'a: foo\nbar', '- foo\n  bar', 'key: a\n  # not comment?\n  b', 'a: b\n  c: d', 'a: x\n  - y', 'plain\nmulti\nline', 'a\n  b: c', 'a: 1 2 3', 'a: --- x', 'a:\n  ---\n', '- ---', 'a: x\n...', 'a: x\n...\n# c', 'a: x # c\n...\n',
  '? a\n: b', '? a\n? b\n: c', '? |\n  block key\n: v', '? - a\n  - b\n: v', '?\n: v', '? a', '?', ':', '- ? a\n  : b', '? a\n:', '? a\n: b\n: c', ': a\n: b', '? a\n  b\n: c', '?a: 1', '? a: 1\n  b: 2\n: v', '? { a: 1 }\n: v', '?\n?\n',
  '- a\n- b', '-\n- b', '- - a\n  - b', '- a: 1\n  b: 2', 'key:\n- a\n- b', 'key:\n  - a\n  -b', '- - - a', '-  a\n-   b', '- a\n  # c\n- b', '-\n  a: 1\n-\n  - x', '- a:\n  - b', '- [a]\n- {b: c}', 'a:\n  - b\n  c: d', '- a\nb: c',
  '- -1', '? -', '-1: x', ':x', 'a:: b', 'a :b', 'a : b', '[a, b]c', "'a'b", '"a"b', "a: 'b'c", 'a: "b" c', 'a: &x', 'a: *x', 'a: @b', 'a: `b', 'a: %b', 'a: |b', 'a: >b', 'a: !b', 'a: ! b', 'a: !', 'a: &', '&', '*', '!', '- &', '[&]', '{*}', 'a: b\n%c', 'a:\n%YAML 1.2', 'a: x\n  %y',
  '['.repeat(50) + ']'.repeat(50), '{a: '.repeat(30) + '1' + '}'.repeat(30), 'a:\n' + Array.from({ length: 20 }, (_, i) => ' '.repeat(2 * i + 2) + 'k' + i + ':').join('\n') + ' leaf',
  'items:\n' + Array.from({ length: 12 }, (_, i) => '  - item' + i).join('\n') + '\n   - bad', Array.from({ length: 9 }, (_, i) => 'k' + i + ': v').join('\n') + '\n bad: 1', Array.from({ length: 98 }, (_, i) => 'k' + i + ': v').join('\n') + '\n bad: 1\nmore: 2\nmore2: 3\nmore3: 4'
)

const seeds = [...configs.slice(0, 40), ...docs.slice(0, 20), 'a: 1\nb: [1, 2]\nc: {d: e}\nf: |\n  text\ng: "q"\nh: \'s\'\n- x\n']
let rngState = 0x2545F491
const random = () => {
  rngState = rngState + 0x6D2B79F5 | 0
  let mixed = Math.imul(rngState ^ rngState >>> 15, 1 | rngState)
  mixed = mixed + Math.imul(mixed ^ mixed >>> 7, 61 | mixed) ^ mixed
  return ((mixed ^ mixed >>> 14) >>> 0) / 4294967296
}
const pick = list => list[Math.floor(random() * list.length)]
const insertions = [' ', '  ', '\t', ':', ': ', '-', '- ', '[', ']', '{', '}', '"', "'", '#', '&a ', '*a', '!', '!!int ', '|', '>', '\n', '\n ', '%', '@', ',', '?', '? ', 'é', '😀', '<<: ', '\r\n', '\\', '---\n', '...\n']
for (let index = 0; index < 900; index++) {
  const seed = pick(seeds)
  if (!seed.length) continue
  const position = Math.floor(random() * seed.length)
  const mode = Math.floor(random() * 5)
  let mutant
  if (mode === 0) mutant = seed.slice(0, position) + seed.slice(position + 1)
  else if (mode === 1) mutant = seed.slice(0, position) + pick(insertions) + seed.slice(position)
  else if (mode === 2) mutant = seed.slice(0, position) + pick(insertions) + seed.slice(position + 1)
  else {
    const lines = seed.split('\n')
    const line = Math.floor(random() * lines.length)
    if (mode === 3) lines[line] = ' ' + lines[line]
    else lines.splice(line, 0, lines[line])
    mutant = lines.join('\n')
  }
  add(mutant)
}

function outcome (load, input) {
  let result
  try {
    result = load(input)
  } catch (error) {
    return { error: error.message, errorName: error.name }
  }
  if (result === undefined) return { empty: true }
  return { output: JSON.parse(JSON.stringify(result)) }
}

const hasLoneSurrogate = text => !text.isWellFormed() || /\\ud[89ab][0-9a-f]{2}(?!\\ud[c-f])|(?<!\\ud[89ab][0-9a-f]{2})\\ud[c-f]/i.test(text)
const seen = new Set()
const entries = []
for (const input of inputs) {
  if (seen.has(input)) continue
  seen.add(input)
  let entry
  try {
    entry = Object.assign({ input }, outcome(text => jsYaml3.safeLoad(text), input), { jsYaml4: outcome(text => jsYaml4.load(text), input) })
  } catch (error) {
    continue
  }
  if (hasLoneSurrogate(JSON.stringify(entry))) continue
  entries.push(entry)
}
fs.writeFileSync(OUT, JSON.stringify(entries, null, 1) + '\n')
console.log('entries', entries.length, 'errors', entries.filter(entry => entry.error).length, 'js-yaml 4 errors', entries.filter(entry => entry.jsYaml4.error).length)
