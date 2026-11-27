/*---
description: >
  A function declaration in a case/default clause is processed with B.3.2 semantics in sloppy
  Annex B mode: it leaks a var binding to the enclosing var scope.
flags: [noStrict]
---*/

switch (1) {
  case 1:
    function sw() {}
}
assert.sameValue(typeof sw, 'function');

switch (2) {
  case 1:
    function notTaken() {}
}
assert.sameValue(typeof notTaken, 'undefined');

let sc;
switch (1) {
  case 1:
    function sc() {}
}
assert.sameValue(typeof sc, 'undefined');

switch (1) {
  case 1:
    function d1() {
      return 1;
    }
  default:
    function d2() {
      return 2;
    }
}
assert.sameValue(d1(), 1);
// Every function declaration in the CaseBlock gets a legacy var binding, not just the matched one.
assert.sameValue(typeof d2, 'function');

function inFunction() {
  switch (1) {
    case 1:
      function inner() {
        return 3;
      }
  }
  return typeof inner;
}
assert.sameValue(inFunction(), 'function');
