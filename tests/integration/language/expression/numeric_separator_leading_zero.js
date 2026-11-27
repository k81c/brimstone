/*---
description: >
  Numeric separators are allowed in the fraction and exponent of decimal literals whose integer
  part is `0`, but never immediately after a leading zero in the integer part.
---*/

assert.sameValue(0.5_5, 0.55);
assert.sameValue(0e1_0, 0);
assert.sameValue(0.5e1_0, 5000000000);
assert.sameValue(10.5_5, 10.55);
assert.sameValue(0.5_5e1_0, 5500000000);
assert.sameValue(0n, 0n);
assert.sameValue(1_0n, 10n);
assert.sameValue(0x1_0, 16);
assert.sameValue(0o1_0, 8);
assert.sameValue(0b1_0, 2);

assert.throws(SyntaxError, function () {
  eval('0_1');
});
assert.throws(SyntaxError, function () {
  eval('0.5__5');
});
assert.throws(SyntaxError, function () {
  eval('0e1_');
});
