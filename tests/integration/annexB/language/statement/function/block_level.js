/*---
description: >
  Annex B B.3.2: block-level function declarations create a var binding in the enclosing var
  scope, initialized when the block is entered, and skipped on lexical conflicts.
flags: [noStrict]
---*/

assert.sameValue(typeof beforeFn, 'undefined');
{
  assert.sameValue(typeof beforeFn, 'function');
  function beforeFn() {}
}
assert.sameValue(typeof beforeFn, 'function');

{
  {
    function nestedFn() {}
  }
}
assert.sameValue(typeof nestedFn, 'function');

{
  function dup() {
    return 1;
  }
  function dup() {
    return 2;
  }
}
assert.sameValue(dup(), 2);

function overwritten() {
  return 1;
}
assert.sameValue(overwritten(), 1);
{
  function overwritten() {
    return 2;
  }
}
assert.sameValue(overwritten(), 2);

let conflict;
{
  function conflict() {}
}
assert.sameValue(typeof conflict, 'undefined');

{
  async function asyncFn() {}
}
assert.sameValue(typeof asyncFn, 'undefined');

{
  function* genFn() {}
}
assert.sameValue(typeof genFn, 'undefined');

// A parameter (or a var sharing its slot) keeps its value; no legacy binding is created.
function withParam(x) {
  {
    function x() {
      return 9;
    }
  }
  return x;
}
assert.sameValue(withParam(1), 1);

function withParamFn(x) {
  function x() {
    return 1;
  }
  {
    function x() {
      return 9;
    }
  }
  return x();
}
assert.sameValue(withParamFn(function () {
  return 2;
}), 1);

function withVarParam(x) {
  var x;
  {
    function x() {
      return 9;
    }
  }
  return typeof x;
}
assert.sameValue(withVarParam(1), 'number');

// A lexical declaration added later in an enclosing block suppresses the legacy var binding.
{
  {
    function laterFn() {}
  }
  let laterFn;
}
assert.sameValue('laterFn' in globalThis, false);
assert.sameValue(typeof laterFn, 'undefined');
