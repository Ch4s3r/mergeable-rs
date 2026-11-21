
const fs = require('fs')
const path = require('path')
const ORIG = process.argv[2]
const OUT = process.argv[3]
require(path.join(ORIG, 'lib/actions/handlebars/populateTemplate'))
const handlebars = require(path.join(ORIG, 'node_modules/handlebars'))
const consts = require(path.join(ORIG, 'lib/configuration/lib/consts'))
const unicodeText = 'stra' + String.fromCharCode(0xdf) + 'e caf' + String.fromCharCode(0xe9) + ' ' + String.fromCharCode(0x2028) + ' ' + String.fromCodePoint(0x1f600)

const results = {
  validationStatus: 'fail', validationCount: 3, passCount: 1, failCount: 1, errorCount: 1,
  failures: [{ name: 'title', status: 'fail' }, { name: 'label', status: 'fail' }],
  validationSuites: [
    { name: 'title', status: 'fail', validations: [
      { status: 'fail', description: 'Title must not include "wip" & <stuff>', details: { input: 'WIP: x', settings: { must_exclude: { regex: 'wip', message: 'no' }, '10': 1, '2': [1, 2.5, null, true] } } },
      { status: 'pass', description: 'ok', details: { input: ['a', 'b'], settings: { min: { count: 1 } } } } ] },
    { name: 'label', status: 'error', validations: [
      { status: 'error', description: 'boom', details: { input: null, settings: {}, error: 'Error: something' } } ] }
  ],
  title: 'Add <login> & "page"', body: 'Line one\nLine two <b>bold</b>\r\nLine three\rFour\n\nSix',
  number: 10, created_at: '2024-01-15T10:00:00Z', completed_at: '2024-07-04T23:59:30.999Z',
  user: { login: 'alice' }, labels: [{ name: 'bug' }, { name: 'feature' }], empty_list: [], empty_object: {},
  zero: 0, negative: -1.5, float: 2.5, big: 12345678901, truthy: true, falsy: false, nothing: null,
  html: '&<>"\x27\x60= /', unicode: unicodeText, nested: { a: { b: { c: 'deep' } }, list: [[1, 2], [3]] },
  keyed: { z: 1, a: 2, '1': 3, '0': 'zero', '-1': 'minus', '01': 'padded' }
}

const corpus = []
const add = (name, template, data = results) => corpus.push({ name, template, data })

for (const key of Object.keys(consts)) {
  const value = consts[key]
  if (Array.isArray(value)) value.forEach((entry, index) => { const payload = entry && entry.payload; if (payload) for (const field of ['title', 'summary', 'text', 'body']) if (typeof payload[field] === 'string') add('consts ' + key + ' ' + index + ' ' + field, payload[field]) })
}
add('checks docs text', "{{#each validationSuites}}\n#### {{{statusIcon status}}} Validator: {{toUpperCase name}}\n{{#each validations }} * {{{statusIcon status}}} ***{{{ description }}}***\n       Input : {{{details.input}}}\n       Settings : {{{displaySettings details.settings}}}\n       {{/each}}\n{{/each}}")
add('merge commit title', '{{{ title }}} (#{{{ number }}})')
add('merge commit message', '{{{ body }}}')
add('status placeholder', 'Your run has returned the following status: {{status}}', { status: 'pass' })
add('format date created', 'created {{formatDate created_at}} done {{formatDate completed_at}}')
add('format date missing field', '[{{formatDate missing_field}}]')
add('format date variants', '{{#each dates}}{{formatDate this}}|{{/each}}', { dates: ['2024-01-15T00:05:00Z', '2024-01-15T12:00:00Z', '2024-01-15T13:07:00+02:00', '2024-01-15', '2024-06', '2024', '2024-02-30', '2024-13-01', 'not a date', '', '2024-01-15T24:00:00Z', '2024-01-15T10:00:00.123456Z', '+002024-01-15T00:00:00Z', '0099-01-15T10:00:00Z', 'Mon, 15 Jan 2024 10:00:00 GMT', '2024-01-15T10:00:00+0100'] })
add('escaping set', '{{html}}|{{{html}}}|{{&html}}')
add('value rendering', '{{zero}}|{{negative}}|{{float}}|{{big}}|{{truthy}}|{{falsy}}|{{nothing}}|{{missing}}|{{labels}}|{{empty_list}}|{{empty_object}}|{{nested.list}}|{{user}}')
add('unicode passthrough', '{{unicode}} {{toUpperCase unicode}}')
add('each array data vars', '{{#each labels}}{{@index}}:{{@key}}:{{@first}}:{{@last}}:{{name}};{{/each}}')
add('each object order and data', '{{#each keyed}}{{@key}}={{this}}@{{@index}}{{#if @first}}F{{/if}}{{#if @last}}L{{/if}};{{/each}}')
add('each else on empty', '{{#each empty_list}}x{{else}}empty{{/each}}|{{#each empty_object}}x{{else}}none{{/each}}|{{#each missing}}x{{else}}miss{{/each}}|{{#each title}}x{{else}}str{{/each}}')
add('each nested parent access', '{{#each validationSuites}}{{name}}[{{#each validations}}{{@../index}}.{{@index}} {{../name}} {{status}} {{@root.validationStatus}},{{/each}}]{{/each}}')
add('each with block params', '{{#each labels as |label position|}}{{position}}={{label.name}} {{/each}}')
add('each over primitives', '{{#each items}}<{{this}}>{{.}}{{/each}}', { items: ['a', 1, true, null, 'x<y'] })
add('inverted last separator', '{{#each failures}} {{toUpperCase name}}{{^@last}}, {{/@last}}{{/each}}')
add('if truthiness', '{{#if empty_object}}T{{else}}F{{/if}}{{#if empty_list}}T{{else}}F{{/if}}{{#if zero}}T{{else}}F{{/if}}{{#if zero includeZero=true}}T{{else}}F{{/if}}{{#if title}}T{{/if}}{{#if nothing}}T{{else}}N{{/if}}')
add('unless and else if chain', '{{#unless falsy}}U{{/unless}}{{#if falsy}}A{{else if zero}}B{{else if title}}C{{else}}D{{/if}}{{#if falsy}}A{{else if nothing}}B{{/if}}')
add('with and lookup', '{{#with user}}{{login}} {{../number}}{{/with}}{{#with missing}}x{{else}}nowith{{/with}}|{{lookup user "login"}}|{{lookup labels 1}}|{{lookup missing "x"}}|{{#with user as |u|}}{{u.login}}{{/with}}')
add('block helper missing sections', '{{#user}}{{login}}{{/user}}|{{#truthy}}yes{{/truthy}}|{{#falsy}}no{{else}}inv{{/falsy}}|{{#labels}}{{name}}{{/labels}}|{{^labels}}nolabels{{/labels}}|{{^empty_list}}empty{{/empty_list}}|{{#title}}{{this}}{{/title}}|{{#zero}}z{{this}}{{/zero}}|{{#missing}}m{{else}}nom{{/missing}}')
add('paths', '{{this.title}}|{{./number}}|{{nested/a/b/c}}|{{nested.a.b.c}}|{{[nested].[a]}}|{{labels.[1].name}}|{{labels.length}}|{{title.length}}|{{nested.missing.deep}}|{{@root.number}}|{{keyed.[01]}}')
add('bracket segments with spaces', '{{[foo bar]}}|{{obj.[a-b]}}|{{a-b}}', { 'foo bar': 'spaced', obj: { 'a-b': 'dash' }, 'a-b': 'top' })
add('whitespace control', 'a  {{~title~}}  b\n  {{~#if truthy~}}\n  yes  \n{{~/if~}}  c')
add('standalone lines', 'list:\n{{#each labels}}\n- {{name}}\n{{/each}}\nend\n  {{#if truthy}}\n  inside\n  {{else}}\n  never\n  {{/if}}\ndone')
add('standalone crlf', 'a\r\n{{#if truthy}}\r\nb\r\n{{/if}}\r\nc')
add('comments', 'x{{! short }}y\n{{!-- long }} comment --}}\nz {{!-- --}}w\n  {{! standalone }}\nq')
add('escaped mustaches', '\\{{title}} and \\\\{{number}} and \\{{a}}\\{{b}}')
add('standalone each else', '{{#each empty_list}}\nitem\n{{else}}\nnothing\n{{/each}}\n')
add('nested blocks indentation', '{{#each validationSuites}}\n  {{#each validations}}\n    {{#if details.error}}\n    E {{details.error}}\n    {{else}}\n    ok {{status}}\n    {{/if}}\n  {{/each}}\n{{/each}}')
add('breaklines', '{{breaklines body}}|{{{breaklines body}}}|{{breaklines missing}}|{{breaklines html}}')
add('to upper case', '{{toUpperCase validationStatus}} {{toUpperCase html}} {{{toUpperCase html}}}')
add('display settings', '{{displaySettings validationSuites.[0].validations.[0].details.settings}}|{{{displaySettings validationSuites.[0].validations.[0].details.settings}}}|{{{displaySettings missing}}}|{{{displaySettings nothing}}}|{{{displaySettings unicode}}}|{{{displaySettings keyed}}}|{{{displaySettings float}}}')
add('if equals', '{{#ifEquals validationStatus "fail"}}F{{else}}N{{/ifEquals}}{{#ifEquals number 10}}ten{{/ifEquals}}{{#ifEquals missing undefined}}U{{/ifEquals}}{{#ifEquals nothing null}}N{{/ifEquals}}{{#ifEquals user user}}same{{else}}diff{{/ifEquals}}{{#ifEquals number "10"}}loose{{else}}strict{{/ifEquals}}')
add('status icons', '{{statusIcon "pass"}}{{statusIcon "fail"}}{{statusIcon "error"}}{{statusIcon "info"}}|{{statusIcon "bogus"}}|{{statusIcon number}}|{{statusIcon missing}}|{{statusIcon}}')
add('subexpressions and hash', '{{toUpperCase (lookup user "login")}}|{{#if (lookup user "login")}}has{{/if}}|{{statusIcon (lookup failures.[0] "status")}}')
add('literal params', '{{lookup keyed 1}}|{{lookup keyed "z"}}|{{#if true}}t{{/if}}{{#if false}}f{{else}}nf{{/if}}{{#if null}}n{{else}}nn{{/if}}{{#if 0}}z{{else}}nz{{/if}}{{#if "x"}}s{{/if}}')
add('missing simple helper renders empty', '[{{nohelper}}][{{#nohelper}}x{{else}}y{{/nohelper}}]')
add('helper name shadows data', '{{statusIcon}}|{{title}}', { statusIcon: 'data', title: 't' })
add('this in nested if', '{{#each labels}}{{#if name}}{{this.name}}/{{../title}}{{/if}}{{/each}}')
add('object key named length', '{{obj.length}}|{{arr.length}}', { obj: { length: 'own' }, arr: [1, 2, 3] })
add('numbers in data', '{{a}} {{b}} {{c}} {{d}}', { a: 1.0, b: 100, c: 0.5, d: -0 })
add('array of arrays and objects', '{{mixed}}', { mixed: [1, 'a', { b: 1 }, null, [2, [3, null]], true] })
add('triple stash objects', '{{{user}}}|{{{labels}}}|{{{missing}}}|{{{nothing}}}')
add('tilde on else', '{{#if falsy}}\n  a\n{{~else~}}\n  b\n{{/if}}')
add('standalone else', '{{#if falsy}}\na\n{{else}}\nb\n{{/if}}\n')
add('standalone inverse caret', '{{#if falsy}}\na\n{{^}}\nb\n{{/if}}\n')
add('each over object of objects', '{{#each obj}}{{@key}}:{{name}} {{/each}}', { obj: { first: { name: 'one' }, second: { name: 'two' } } })
add('deep depth parent', '{{#each validationSuites}}{{#each validations}}{{#each details.input}}{{this}}<{{../../name}}>{{/each}}{{/each}}{{/each}}')
add('root data in each', '{{#each labels}}{{@root.user.login}}{{/each}}')
add('doc comment example', '- {{formatDate created_at}}\n- {{title}}\n- {{body}}')

const errorCases = [
  ['to upper case of missing', '{{toUpperCase missing}}'],
  ['to upper case of null', '{{toUpperCase nothing}}'],
  ['to upper case of number', '{{toUpperCase number}}'],
  ['to upper case without argument', '{{toUpperCase}}'],
  ['missing helper with param', '{{nohelper title}}'],
  ['missing block helper with param', '{{#nohelper title}}x{{/nohelper}}'],
  ['if without argument', '{{#if}}x{{/if}}'],
  ['each without argument', '{{#each}}x{{/each}}'],
  ['if equals one argument', '{{#ifEquals title}}x{{/ifEquals}}'],
  ['mismatched close', '{{#if title}}x{{/each}}'],
  ['unclosed mustache', '{{title'],
  ['unclosed block', '{{#if title}}x'],
  ['numeric path segment', '{{labels.0}}']
]
for (const [name, template] of errorCases) corpus.push({ name, template, data: results })

for (const entry of corpus) {
  try {
    entry.expected = handlebars.compile(entry.template)(entry.data)
  } catch (error) {
    entry.error = String(error)
  }
}
fs.mkdirSync(path.dirname(OUT), { recursive: true })
fs.writeFileSync(OUT, JSON.stringify(corpus, null, 1) + '\n')
console.log('entries', corpus.length, 'errors', corpus.filter(e => e.error).map(e => e.name + ' => ' + e.error.split('\n')[0]).join('\n'))
