/*---
description: >
  Function declarations in case clauses are ordinary block-scoped declarations in modules; they
  are legal but do not leak a var binding.
flags: [module]
---*/

switch (1) {
  case 1:
    function sw() {}
}
assert.sameValue(typeof sw, 'undefined');

{
  function blocked() {
    return 1;
  }
  assert.sameValue(blocked(), 1);
}
assert.sameValue(typeof blocked, 'undefined');
