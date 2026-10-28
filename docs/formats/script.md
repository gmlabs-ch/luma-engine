# Compiled scripts

Game logic in a Wintermute Engine game is written in a C-like scripting
language and compiled to bytecode by the editors. Games ship the compiled
form: `*.script` files inside the packages that start with the magic bytes
`DE AD C0 DE`. The engine never needs the source; a script that does not
start with the magic is handed to the compiler DLL, which is only present
in the development tools.

Sources: the MIT-licensed [WME Lite](https://github.com/MnemonicWME/wmelite)
sources (`ScScript.cpp`, `ScScript.h`, `dcscript.h`), and analysis of game
files. The semantics of the instructions (what the virtual machine does
with them) are documented with the virtual machine itself; this document
describes the file layout and the instruction encoding.

Implemented by the `luma-script` crate (`luma_script::Program`), and shown
by `luma script <game> <file>`.

## Layout

All integers are little-endian. Offsets are from the start of the file.

| Offset | Size | Content |
|--------|------|---------|
| 0 | 4 | Magic: bytes `DE AD C0 DE`, read as the 32-bit value `0xDEC0ADDE` |
| 4 | 4 | Version, `0x0102` for every script of the WME 1.x era (`1.2`) |
| 8 | 4 | Offset of the code |
| 12 | 4 | Offset of the function table |
| 16 | 4 | Offset of the symbol table |
| 20 | 4 | Offset of the event table |
| 24 | 4 | Offset of the externals table |
| 28 | 4 | Offset of the method table |
| 32 | | The script's own path as a C string (`scripts\scene.script`) |
| | | Code, then the tables, in the order written by the compiler |

The engine refuses a version *greater* than `0x0102`. Versions below
`0x0101` have no externals table. Every script of the games analysed is
version `0x0102`; the writer produces the tables in the order function,
symbol, event, externals, method, each one directly after the code, but a
reader must use the header offsets.

A **string** is a NUL-terminated sequence of 8-bit characters in the game's
Windows code page (usually plain ASCII). There are no escape sequences:
the compiler has already resolved them.

### Symbol table

```
count: u32
count × { index: u32, name: string }
```

Each entry stores the symbol's index explicitly; indexes are below
`count`. Symbols are variable, function and method names referenced by
instructions. In practice the entries are in index order, but a reader
should place each entry at its index.

### Function, event and method tables

Three tables with the same shape:

```
count: u32
count × { position: u32, name: string }
```

`position` is the offset of the first instruction of the body.

- **Functions** (`function name() { ... }`) are called by `CALL` with the
  position resolved by the compiler, so the table is only needed by
  tools and debuggers. Lookup by name is case-sensitive, first match.
- **Events** (`on "LeftClick" { ... }`) are the handlers the engine
  invokes with `ApplyEvent`. Lookup is **case-insensitive**, and the
  **last** matching entry wins.
- **Methods** (`method name() { ... }`) are callable from other scripts
  through the owning object (`obj.name()`). Lookup is case-sensitive,
  first match.

A script with no events and no methods finishes when its top-level code
returns; a script with any of them stays loaded (*persistent*) so that its
handlers can be invoked.

### Externals table

```
count: u32
count × {
  dll_name: string
  name: string
  call_type: u32     ; 0 stdcall, 1 cdecl, 2 thiscall
  return_type: u32   ; see below
  num_params: u32
  num_params × param_type: u32
}
```

Declared with `external "dll" cdecl long name(long, string)` in the source,
these are calls into Windows DLLs. Types: 0 void, 1 bool, 2 long, 3 byte,
4 string, 5 float, 6 double, 7 membuffer. Luma Engine reads the table but
does not call DLLs; an `EXTERNAL_CALL` whose name is in this table returns
`null`, as WME Lite does on non-Windows platforms. The games analysed
declare none.

## Code

The code is a sequence of instructions from the code offset up to the
first table. Every instruction starts with a 32-bit opcode, followed by
an operand whose size depends on the opcode:

| Operand | Encoding |
|---------|----------|
| none | |
| symbol | `u32`, an index into the symbol table |
| address | `u32`, an offset in the file |
| int | `u32`, read as a signed 32-bit integer |
| bool | `u32`, zero is false |
| float | `f64`, 8 bytes, IEEE 754 little-endian |
| string | NUL-terminated string, inline |

So instructions are not fixed-size, and only `PUSH_FLOAT` and
`PUSH_STRING` can make an instruction longer than 8 bytes.

| Opcode | Mnemonic | Operand | Effect |
|--------|----------|---------|--------|
| 0 | `DEF_VAR` | symbol | Define a local variable (`var x`) in the current scope, or in the script's globals outside any scope; initial value `null` |
| 1 | `DEF_GLOB_VAR` | symbol | Define an engine-wide global (`global x`) if it does not exist yet |
| 2 | `RET` | | Return from a function, or end the script |
| 3 | `RET_EVENT` | | End an event handler or method body |
| 4 | `CALL` | address | Push the return address and jump |
| 5 | `CALL_BY_EXP` | | Call a method: pops the method name, then the object |
| 6 | `EXTERNAL_CALL` | symbol | Call a built-in or external function by name |
| 7 | `SCOPE` | | Push a new variable scope |
| 8 | `CORRECT_STACK` | int | Adjust the pushed arguments to the expected count |
| 9 | `CREATE_OBJECT` | | Push a new empty object (`new Object()`) |
| 10 | `POP_EMPTY` | | Discard the top of the stack |
| 11 | `PUSH_VAR` | symbol | Push the value of a variable |
| 12 | `PUSH_VAR_REF` | symbol | Push a reference to a variable |
| 13 | `POP_VAR` | symbol | Pop into a variable |
| 14 | `PUSH_VAR_THIS` | | Push the current `this` |
| 15 | `PUSH_INT` | int | Push an integer |
| 16 | `PUSH_BOOL` | bool | Push a boolean |
| 17 | `PUSH_FLOAT` | float | Push a float |
| 18 | `PUSH_STRING` | string | Push a string |
| 19 | `PUSH_NULL` | | Push `null` |
| 20 | `PUSH_THIS_FROM_STACK` | | Push a reference to the top of the stack onto the `this` stack |
| 21 | `PUSH_THIS` | symbol | Push a reference to a variable onto the `this` stack |
| 22 | `POP_THIS` | | Pop the `this` stack |
| 23 | `PUSH_BY_EXP` | | Pop a property name, then an object; push the property |
| 24 | `POP_BY_EXP` | | Pop a property name, an object and a value; set the property |
| 25 | `JMP` | address | Jump |
| 26 | `JMP_FALSE` | address | Pop; jump if false |
| 27 | `ADD` | | Pop two, push the sum (string concatenation if either is a string) |
| 28 | `SUB` | | Pop two, push the difference |
| 29 | `MUL` | | Pop two, push the product |
| 30 | `DIV` | | Pop two, push the quotient (always a float) |
| 31 | `MODULO` | | Pop two, push the integer remainder |
| 32 | `NOT` | | Pop, push the logical negation |
| 33 | `AND` | | Pop two, push the logical and |
| 34 | `OR` | | Pop two, push the logical or |
| 35 | `CMP_EQ` | | Pop two, push `a == b` |
| 36 | `CMP_NE` | | Pop two, push `a != b` |
| 37 | `CMP_L` | | Pop two, push `a < b` |
| 38 | `CMP_G` | | Pop two, push `a > b` |
| 39 | `CMP_LE` | | Pop two, push `a <= b` |
| 40 | `CMP_GE` | | Pop two, push `a >= b` |
| 41 | `CMP_STRICT_EQ` | | Pop two, push `a === b` (same type and value) |
| 42 | `CMP_STRICT_NE` | | Pop two, push `a !== b` |
| 43 | `DBG_LINE` | int | Source line of the following instructions, for errors and the debugger |
| 44 | `POP_REG1` | | Pop into the auxiliary register (used by `switch`) |
| 45 | `PUSH_REG1` | | Push the auxiliary register |
| 46 | `DEF_CONST_VAR` | symbol | Define an engine-wide global constant (`const x`) |

An unknown opcode is a fatal error for the script: the engine logs it and
stops that script.

### Patterns worth knowing

- **Calls.** For `obj.Method(a, b)` the compiler pushes `b`, then `a`,
  then the argument count, then the object and the method name, and emits
  `CALL_BY_EXP`. A callee starts with `CORRECT_STACK n`, which trims or
  pads the arguments to its declared parameter count, then pops each
  parameter into a local. Built-in functions (`Sleep`, `Random`, ...) use
  the same argument layout with `EXTERNAL_CALL`. A call always leaves a
  result on the stack; statements that ignore it emit `POP_EMPTY`.
- **Entry points.** The top-level code runs from the code offset and ends
  with `RET`; the compiler places the handler bodies after it and jumps
  over them (`JMP` to the final `RET`), so a listing shows the handlers
  between the last top-level statement and the end.
- **`this`.** A method call pushes the object onto the `this` stack
  (`PUSH_THIS` or `PUSH_THIS_FROM_STACK`) for the duration of the call and
  pops it with `POP_THIS` afterwards.
- **Includes.** `#include` is resolved by the compiler, so a compiled
  script contains everything its includes declared: a game whose scripts
  all include the same file of `global` declarations repeats those
  `DEF_GLOB_VAR` instructions in every script. Defining a global that
  already exists is a no-op, so this is harmless.
- **Line numbers.** `DBG_LINE` appears before the first instruction of
  each source line; the engine uses it in runtime error messages. Tools
  can use it to map code back to the source.

## Example

The header of a scene script of The White Chamber, as `luma script`
prints it:

```
scripts\scene.script: version 1.2, 4102 bytes
  code at 53..1908 (1855 bytes)
  122 symbol(s), 0 function(s), 1 event(s), 0 method(s), 0 external(s)
  event LeftClick at 1789
```

## Notes for implementers

- Check every offset and every string against the file size before
  reading; a truncated or corrupt script must be an error, not a panic.
- Resolve symbol indexes against the table size when decoding, so that an
  invalid index is reported with its instruction offset.
- Keep the raw bytes: event threads are created by copying the program
  and jumping to the handler's position, so the program is immutable
  shared data.
- A disassembler should label the positions listed in the function, event
  and method tables; they are the only entry points besides the code
  offset.
