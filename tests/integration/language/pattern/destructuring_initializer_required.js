/*---
description: >
  A destructuring declarator requires an initializer in every declaration kind and in for heads,
  but not in for-in/for-of heads or catch parameters.
---*/

var invalid = [
  'var [a];',
  'var {a};',
  'var {a = 1};',
  'let [a];',
  'let [a = 1];',
  'let {a};',
  'let {a = 1};',
  'const [a];',
  'const {a};',
  'for (var [a];;) {}',
  'for (let [a];;) {}',
  'for (const [a];;) {}',
];

for (var source of invalid) {
  assert.throws(SyntaxError, function () {
    eval(source);
  }, source);
}

// Controls: patterns in contexts where no initializer is expected remain legal.
let [a] = [];
let {b} = {b: 1};
assert.sameValue(a, undefined);
assert.sameValue(b, 1);

var iterated = 0;
for (var [c] of [[1], [2]]) {
  iterated += c;
}
assert.sameValue(iterated, 3);

var caught;
try {
  throw [42];
} catch ([e]) {
  caught = e;
}
assert.sameValue(caught, 42);
