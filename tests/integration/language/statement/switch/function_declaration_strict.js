/*---
description: >
  Function declarations in case clauses are ordinary block-scoped declarations in strict code;
  they are legal but do not leak a var binding.
flags: [onlyStrict]
---*/

switch (1) {
  case 1:
    function sw() {}
}
assert.sameValue(typeof sw, 'undefined');

function inFunction() {
  var seen;
  switch (1) {
    case 1:
      function inner() {
        return 3;
      }
      seen = inner();
  }
  return [seen, typeof inner];
}
var result = inFunction();
assert.sameValue(result[0], 3);
assert.sameValue(result[1], 'undefined');
