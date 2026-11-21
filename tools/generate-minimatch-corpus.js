
const fs = require('fs'); const path = require('path')
const ORIG = process.argv[2]; const OUT = process.argv[3]
const minimatch = require(path.join(ORIG, 'node_modules/minimatch'))
const files = ['bug', 'feature', 'anotherLabel2', 'label1', 'Label2', 'testLabel', '.hidden', 'aab', 'a2', 'ac', 'a/b', 'ab', 'src/x/a.js', 'src/a.js', 'a.JS', 'x?y', 'xzy', '', 'size/L', 'a.b', 'axb', 'foo.txt', 'foo.js', '#x', 'a', 'a/', 'x[undefined', 'work in progress', 'size: XL', 'a b', 'a/.b/c', 'a/b/c', 'priority: high', 'x(y)', 'x|y', 'a{b}c', 'a1', 'a3', 'a4', '[x]', 'xa', 'a-b', 'é']
const patterns = ['bug', '*Label*', '*', '.*', '!feat*', '!!bug', '#x', ' #x', '+(a)b', 'a{1..3}', 'a{b,c}', 'a//b', 'a[!c]', 'a[^c]', 'src/*.js', 'src/**/*.js', '**/*.js', '*.js', 'x\\?y', '', 'size/*', 'a.b', '!(*.js)', '*.!(js)', 'a/**', 'a/**/c', '**', '*[', '[', 'a?', '?', 'work*', 'size: *', 'a b', '@(bug|feature)', '*(a|b)', '?(a)b', 'x(y)', 'x|y', 'a{b}c', '[z-a]', '[a-c]', 'a[0-9]', '\\*', '[x]', '\\[x\\]', 'a/', 'priority: {high,low}', '{a,b}*', 'a-b', '?(x)a', 'a/*', ' bug ', '*é*', '[[]x]']
const entries = []
for (const pattern of patterns) for (const file of files) {
  let outcome
  try { outcome = { matches: minimatch(file, pattern) } } catch (error) { outcome = { error: String(error) } }
  entries.push(Object.assign({ file, pattern }, outcome))
}
fs.writeFileSync(OUT, JSON.stringify(entries) + '\n')
console.log('entries', entries.length)
