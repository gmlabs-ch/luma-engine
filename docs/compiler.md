# The script compiler

This document specifies the compiler that turns Wintermute Engine script
sources (`*.script`, `*.dcs`, with `*.inc` includes) into the compiled form
described in [formats/script.md](formats/script.md), precisely enough to
write a compiler that produces **byte-for-byte the same output** as the
original one. The `luma-compiler` crate (`luma_compiler::Compiler`) and
the `luma compile` command implement it; see
[Implementation](#implementation-in-luma) at the end for the crate's shape,
its few deliberate deviations, and how it is tested.

The instruction names are those of `docs/formats/script.md` (`DEF_VAR`,
`PUSH_INT`, ...); the Rust names in `luma_script::Opcode` and
`luma_script::Instruction` are the same words in camel case (`DefVar`,
`PushInt`, ...).

## Sources and how sure we are

Read for this document (all MIT-licensed, WME 1.x tree, paths relative to
`src/`):

- `compiler/lang.l` (lexer), `compiler/lang.y` (grammar),
  `compiler/parse.c`, `compiler/parse.h`, `compiler/parse.output`
  (Bison 1.24 output: tables, rule numbers, conflict report),
  `compiler/lex.c` (Flex 2.5 output, checked against `lang.l`);
- `compiler/main.c`, `main.h` (entry points, include callback, BOM);
- `compiler/tree.h`, `tree.c` (AST, where line numbers are captured);
- `compiler/weed.c`, `symbol.c`, `symbol.h` (checks and symbol tables);
- `compiler/opcode.c`, `opcode.h` (code generator and file writer);
- `compiler/error.c`, `memory.c`;
- `engine_core/wme_base/dcscript.h` (opcodes, version, type enums),
  `ScEngine.cpp` (how the engine calls the compiler),
  `BGame.cpp` (`PublishNatives`), `engine_core/wme_ad/AdGame.cpp`,
  `BFileManager.cpp`, `BPkgFile.cpp` (include loading), `ScScript.cpp`
  (how the tables are read);
- `ProjectMan/FilterScript.cpp`, `wme_comp/main.cpp` (how the editor tools
  call the compiler).

WME Lite (`github.com/MnemonicWME/wmelite`) contains no compiler source;
see [WME Lite and WME 1.x](#wme-lite-and-wme-1x).

Every statement below carries an evidence mark:

- **[S]** read from the compiler source.
- **[P]** also exercised by running a port of the pipeline on small inputs.
  The port drives the original Bison tables, so what it accepts, how it
  groups operators and which lines it records are the original's by
  construction; this mark is mostly used for grammar facts.
- **[V]** also verified: the same port reproduced all 276 compiled scripts
  of The White Chamber 1.7 DE for which the source pack has the matching
  `.script` text, byte for byte (the 276 scripts include 599 `#include`
  expansions, 578 events, 47 functions and methods, 108 `switch`, 96
  `while`, 2 `for`, 454 `if`, 1393 `break`), and so does `luma-compiler`
  (288 of 288 when the 12 scripts of `language.dcp` are counted too). The
  source pack is a local test asset (see `CLAUDE.md`, game data rule): the
  comparison is an opt-in test that skips when it is absent
  (`crates/luma-compiler/tests/white_chamber.rs`), and the tests that
  always run derive their expected listings from this document.
- **[D]** derived by reading the sources, not exercised by that corpus
  (constructs absent from it: `continue`, `new`, float literals, `*`, `%`,
  `===`/`!==`, `const`, `external`, string escapes, `/* */` comments,
  `#region`, bytes above 0x7F, `for` with several updates, initialised
  multi-name declarations, a `break` or `continue` after a nested loop or
  `switch`, `case` bodies using `continue`, assignments inside expressions).
  `luma-compiler` implements these from this text; its tests check them
  against listings derived here, and their output was also compared with
  the port's on small snippets (identical, see
  [Implementation](#implementation-in-luma)), which settles what the
  original's *source* does but not what a shipped binary might differ in.

The White Chamber ships its scripts compiled by this compiler with version
`0x0102` throughout (288 scripts), as does OpenQuest (64 scripts); both use
the table order described below.

## Overview of the pipeline [S]

`CompileFile(name)` (`main.c`) does, in this order, and stops at the first
phase that reported an error:

1. read the file through the engine's file manager (`LoadFile` callback);
   skip a UTF-8 byte order mark (`EF BB BF`, only if the file is longer than
   3 bytes); set the line counter to 1;
2. lex and parse (Flex + Bison, `lang.l`, `lang.y`), building the AST. The
   parser fetches tokens lazily and AST nodes record the lexer's current
   line **at the moment their production is reduced**; this matters for
   `DBG_LINE` (see [Line numbers](#line-numbers));
3. weed (`weed.c`): checks for `break`/`continue`/`return` placement, and
   splits multi-name declarations;
4. symbol passes (`symbol.c`): two passes, declarations and references;
5. code generation (`opcode.c`): one pass over the tree, writing the file
   into a growing buffer with back-patching, then the tables.

Any reported error (lexical, syntax, weed, symbol) makes the compile fail
and produces no output. Errors are reported with a line number and a
message through a callback; the messages are listed in
[Diagnostics](#diagnostics).

Inputs of the compiler besides the text: the **file name** written into the
header, the **include loader** (name to bytes), and the **list of native
functions and variables** that the host registers (`DefineFunction`,
`DefineVariable`); see [Natives](#natives).

## Lexical structure

The scanner is Flex-generated from `lang.l` [S]; the checked-in `lex.c` is
generated from the same `lang.l` (the `this`, `++` and `--` rules are
commented out in both) [S]. Longest match wins; on equal length the rule
listed first in `lang.l` wins. Input is 8-bit: every byte from 0x80 to 0xFF
is an ordinary character, no re-encoding happens anywhere, and a file is
treated as bytes in the game's code page [S].

### Whitespace, newlines, comments

- Blanks are space and tab. A newline is `\r\n` or `\n`. A lone `\r` outside
  a string or comment is "Invalid character" [S].
- `// ...` runs to the end of the line (the newline itself is not part of
  it).
- `/* ... */` comments **nest** (`/*` inside a comment increments a depth
  counter, `*/` decrements it); a `//` inside a comment swallows the rest of
  that line, including any `*/` on it. Newlines inside comments are counted.
  An unterminated comment is the error "Unclosed comment." [S]
- `#region` and `#endregion`, wherever a token may start, swallow the rest
  of the line (whatever follows the word, including more letters). Any other
  `#` that does not begin `#include` is "Invalid character" [S].

### Line counter

The scanner increments the global line number for every newline it
consumes **only while the include depth is 0**, i.e. only in the main file:
newlines inside included files and inside comments of included files are not
counted [S][V]. Consequences: every token that comes from an included file
carries the line of the `#include` directive's line (the counter is frozen
while the include is read); the newline that ends the `#include` line is
consumed after the include's end of file, so the first token after the
directive is on the next line as expected. The counter starts at 1 and,
after the whole input has been scanned, equals 1 plus the number of newlines
of the main file; this final value matters (see [Line numbers](#line-numbers)).

### Tokens

Keywords (case-sensitive; all other letter sequences are identifiers, and a
longer identifier always beats a keyword prefix: `format` is an identifier,
not `for` + `mat`) [S][P]:

| Class | Words |
|-------|-------|
| statements | `if else while for switch case default return function method var global const on new null` |
| literals | `true false` (boolean constants) |
| types, only meaningful in `external` declarations but reserved everywhere | `bool long short byte string float double int membuffer` |
| calling conventions (reserved everywhere) | `stdcall cdecl thiscall` |
| declarations | `external` |
| `break;` and `continue;` | each is **one token that includes the semicolon** (`break;`, `continue;`) [S][P]; `break ;` (a space) or `break` without a semicolon lexes as the identifier `break` |

Notes: `short` and `int` both become `long`. `this`, `self`, `Game`, `Math`,
`Directory` are ordinary identifiers (the host declares them as variables,
see [Natives](#natives)); there is **no** `this` keyword, no `do`, no `++`,
no `--` and no compound assignment [S]. The reserved words, type words
included, cannot be used as variable or member names (`x.string`,
`x.default` and `x.long` are syntax errors).

Operators and punctuation: `&&` `||` `==` `===` `!=` `!==` `>=` `<=` `=` `>`
`<` `!` `+` `-` `*` `/` `%` `{` `}` `;` `(` `)` `,` `.` `[` `]` `:` [S].
There is no unary `+`, no `&`, `|`, `^`, `~`, `?`, `<<`.

Identifier: `[A-Za-z_][A-Za-z0-9_@]*` (the `@` is allowed after the first
character) [S].

Integer constant: `0` or `[1-9][0-9]*` (decimal only; no sign, no hex).
`007` lexes as `0`, `0`, `7`, i.e. a syntax error. The value is `atoi` of
the text; behaviour above `INT_MAX` is the C runtime's (MSVC `atoi`
saturates in practice, not verified; see [Open questions](#open-questions))
[S].

Float constant: `[0-9]+\.[0-9]*` (`5.` is valid and equals 5.0; `.5`, `1e5`
and `5.5.5` are not: the second is `.` + `5`, the third `1` + identifier
`e5`). The value is `atof(text)`, a C `double` [S].

String constant: `"` ... `"`, on one line. Escapes, the only ones: `~n`
(newline byte 0x0A), `~~` (a tilde), `~"` (a double quote). Any other
character, including a backslash or a `~` followed by something else, is
kept as is (`~x` stays the two characters `~x`). A newline before the
closing quote is "Unterminated string constant"; more than 4096 characters
is "String constant too long" [S][P]. There are no `\` escapes, no
concatenation of adjacent literals.

### `#include`

`#include` followed by blanks and a double-quoted name, anywhere a token may
start (it is a lexer rule, so it works mid-statement or mid-function too):

```
#include "scripts\base.inc"
```

- Anything else than blanks and a quoted name after `#include` is "Include
  filename expected" [S]; a newline right after the word is not an error in
  the original (its scanner has no rule for it there, so it is dropped
  without being counted, and the name is read from the next line), a
  corner the Luma compiler reports as an error instead.
- The name is the text between the quotes, **verbatim** (backslashes are not
  escapes), and is passed as is to the host's file loader. It is not
  resolved relative to the including file: the host resolves it like any
  other game file, i.e. relative to the game (project) root, trying the disk
  first, then the packages; package lookup is case-insensitive and treats
  `/` and `\` alike [S] (`BFileManager::OpenFileRaw`, `BPkgFile::Open`).
  Extensions are irrelevant (`.inc` is a convention).
- A BOM at the start of the included file is skipped (if longer than 3
  bytes) [S]. The text is spliced into the token stream at that point; at
  the end of the included file the scanner resumes right after the
  directive [S][V].
- Nesting is limited: more than 20 levels is "Includes nested too deeply
  (20)" (the original's include stack array has exactly 20 entries, so the
  last permitted level writes one element past it; treat 19 as the practical
  limit) [S].
- There is **no include guard and no duplicate suppression**: including the
  same file twice pastes it twice, and if it declares anything (`global`,
  `var`, `const`, `function`, `method`) the second paste fails with
  "Duplicate declaration ..." (see [Symbols](#symbols-scoping-and-checks))
  [S]. Games avoid this by structuring their includes (a script includes
  `base.inc`, which includes `const.inc`, and does not include `const.inc`
  itself). The same `global` declarations repeated in *different* scripts are
  fine, each script being compiled on its own.
- A missing file is "Error opening include file <name>" [S].

## Grammar

The grammar is `lang.y` compiled by Bison 1.24 with the precedence
declarations below; conflicts are resolved by Bison's usual rules (shift
beats reduce; between two reductions the earlier rule wins). `parse.output`
lists 126 rules and the conflicts: dangling `else` (shift), toplevel
statement runs (shift: consecutive statements form one run), and the empty
alternatives of `switch` (see below). Anything not derivable from the
grammar below is a syntax error; the parser stops at the first one (there is
no error recovery rule) [S].

### EBNF (a faithful restatement)

```
script      = { toplevel } ;
toplevel    = function | event | external | statement { statement } ;   (* a run of statements *)

function    = ( "function" | "method" ) IDENT "(" [ formal { "," formal } ] ")" block ;
formal      = [ "var" ] IDENT ;
event       = "on" STRING statement ;
external    = "external" STRING [ callconv ] [ exttype ] IDENT
              "(" [ extparam { "," extparam } ] ")" ";" ;
callconv    = "stdcall" | "cdecl" | "thiscall" ;           (* default stdcall *)
exttype     = "bool" | "long" | "short" | "int" | "byte" | "string"
            | "float" | "double" | "membuffer" ;           (* default void *)
extparam    = exttype [ IDENT ] ;

block       = "{" { statement } "}" ;
statement   = ";"
            | "return" [ exp ] ";"
            | "if" "(" exp ")" statement [ "else" statement ]
            | "while" "(" exp ")" statement
            | "for" "(" forinit { "," forinit } ";" exp ";" [ exp { "," exp } ] ")" statement
            | block
            | decl
            | exp ";"
            | BREAK                       (* the token "break;" *)
            | CONTINUE                    (* the token "continue;" *)
            | "switch" "(" exp ")" "{" [ case { case } ] [ default ] "}" ;
decl        = ( "var" | "global" | "const" ) IDENT { "," IDENT } [ "=" exp ] ";" ;
forinit     = "var" IDENT [ "=" exp ] | exp ;
case        = "case" postfix ":" { statement } ;
default     = "default" ":" statement { statement } ;       (* last, body not empty *)

exp         = lvalue "=" exp
            | exp binop exp
            | "null"
            | unary ;
unary       = "-" unary | "!" unary | "(" ( "var"|"global"|"const" ) ")" unary | postfix ;
postfix     = INT | FLOAT | BOOL | STRING
            | lvalue
            | "(" exp ")"
            | IDENT "(" [ args ] ")"                       (* call *)
            | IDENT "." IDENT "(" [ args ] ")"             (* method call, fixed name *)
            | IDENT "[" exp "]" "(" [ args ] ")"           (* method call, computed name *)
            | "new" IDENT [ "(" [ args ] ")" ] ;
args        = exp { "," exp } ;
lvalue      = IDENT | IDENT "." IDENT | IDENT "[" exp "]" ;
```

Points that differ from what one would assume:

- **There is no `do ... while`.** There are no function expressions, no
  nested functions, no arrays or object literals, no `+=`, `++`.
- A `for` loop **must have an initializer list and a condition**:
  `for (init; cond; update)`. Only the update list may be empty. An empty
  initializer (`for (; cond; ...)`) dereferences a null pointer in the
  original (weed pass) and crashes it; there is no reference output.
  Initializers are `var x [= exp]` declarations or expressions (the
  `var` form here takes exactly one name, `forinit` list items are separated
  by commas) [S].
- Declarations take **one shared initializer**: `var a, b = 5;` declares
  both and assigns the expression to both (it is evaluated once per name)
  [S]. The `var` keyword is not an expression; `decl` needs the final `;`.
  `global` and `const` are allowed in any statement position, not just at the
  top level; they compile like `var` with a different opcode.
- Member access is one level only: `a.b` and `a[e]` are the only lvalues;
  `a.b.c = 1`, `a[1][2]`, `a.b[1]` and `f().x` are syntax errors. Calls need
  an identifier on the left: `obj.Method(args)` or `obj[nameExp](args)` or
  `Func(args)`; `a.b.Method()` is a syntax error [S][P].
- `null` is an `exp` alternative, not a `unary`: `x == null` and `f(null)`
  and `(null)` are fine, `-null` and `!null` and `case null:` are syntax
  errors [S][P].
- The label of a `case` is a `postfix` expression: a literal, a variable, a
  member, a call, a parenthesised expression; **not** a negative number
  (`case -1:` is a syntax error, write `case (0-1):`) and not `null` [S][P].
- A `switch` body is zero or more `case`s (each body may be empty), then an
  optional `default` which must be the last item and must contain at least
  one statement (`default:` alone before `}` is a syntax error) [S][P].
- `event` takes any single statement, not necessarily a block
  (`on "X" Sleep(1);` is valid) [S].
- `function` and `method` have identical syntax; the only difference is the
  table the name goes to. The `var` before a parameter is optional.
- `break;`/`continue;` are single tokens including the `;`, so a statement
  `break` without the semicolon is a reference to a variable named `break`
  (an "undefined variable" error unless declared).
- Casts: `( var ) x` is parsed (type word `var`, `global` or `const` between
  parentheses) and produces **no code** at all, not even for `x` (see the
  expression table). It is meaningless and included for completeness
  [S][P].
- Dead productions: `x++`, `x--` (never lexed), `this[...]` (never lexed),
  hence `PUSH_VAR_THIS` is never emitted by this compiler [S].

### What the semantic actions build [S]

Apart from the obvious node per production, these details matter for the
output:

- **Argument lists are reversed at parse time** (`swapArguments`) for calls,
  method calls and `new`: the stored list is last-argument-first. Lists that
  are not reversed keep source order: the `for` initializer list, the `for`
  update list, the `case` list, external parameter lists. The expression
  generator follows each node's `next` link after generating it (see below).
- `a.b(...)` stores the method name as a **string-constant node** created in
  the same reduction as the call (so it has the call's line); `a.b` as an
  lvalue stores `b` the same way, in the lvalue's reduction.
- `if`/`else` bodies and `for` bodies are wrapped in a scope node and a
  braced block is wrapped in one too; scopes have no effect on the output
  (weed removes a directly nested pair).
- A run of `statement`s is a left-nested sequence node; no effect on the
  output.
- In a `switch`, the `default` branch (if any) is appended after the last
  `case`; syntactically it can only be last anyway.
- `short`/`int` in an `external` declaration are stored as `long`; the
  parameter names of an `external` are discarded; a missing calling
  convention is `stdcall`, a missing return type is `void`.
- `true` is stored as 1, `false` as 0.

### Precedence and associativity [S][P]

From the lowest to the highest (a line is one level):

| Level | Operators | Associativity |
|-------|-----------|---------------|
| 1 | `=` (only as `lvalue = exp`) | right |
| 2 | `&&` `||` (**the same level**) | left |
| 3 | `==` `!=` `===` `!==` `<` `>` `<=` `>=` (**the same level**) | left |
| 4 | binary `+` `-` | left |
| 5 | `*` `/` | left |
| 6 | `%` (**binds tighter than `*` and `/`**) | left |
| 7 | unary `-` `!` and the cast | prefix |

Consequences, checked on the generated automaton:

- `a || b && c` parses as `(a || b) && c`; `a && b || c` as `(a && b) || c`.
- `a == b < c` parses as `(a == b) < c`; `a < b == c` as `(a < b) == c`.
- `a * b % c` parses as `a * (b % c)`; `a % b * c` as `(a % b) * c`.
- `a + b = c` parses as `a + (b = c)`: an assignment is an `exp`, so it can
  be the right operand of any binary operator; `a = b = c` is
  `a = (b = c)`; `if (a = b)` is valid and assigns.
- `-a * b` is `(-a) * b`; `!a == b` is `(!a) == b`.
- Argument lists are `exp` lists; the empty list is allowed.
- Dangling `else` attaches to the nearest `if`.

Operator results are the virtual machine's business; the compiler never
folds constants and never reorders operands.

## Symbols, scoping and checks

These phases only decide whether the program is valid and whether a called
name is a native or a script function; they emit nothing [S].

**Natives** are registered by the host before compiling; see
[Natives](#natives). They form the *root table* together with every name
declared at the top level of the script.

**Symbol tables.** The root table is created with the natives (functions and
variables). Then pass 1 walks the top-level items **in source order**:

- `function`/`method NAME`: if `NAME` already exists in the root table
  (native, earlier function or method, `external`, or top-level variable)
  the error is "Duplicate declaration of function 'NAME'" and the body is
  skipped; otherwise `NAME` is added as a function and a child table is
  created for the body.
- `external ... NAME`: same duplicate check, adds `NAME` as a (native-like)
  function.
- `on "..." stm`: a child table for the body; the event name is not a symbol.
- top-level statement runs: declarations go into the **root table**.

Inside a function or event body, parameters and every `var`/`global`/`const`
declaration (at any nesting level, including `for` initializers) go into
that function's table: **blocks do not open scopes at compile time**.
Declaring a name twice in the same table (same function body, or twice at
the top level) is "Duplicate declaration of variable 'x'" (or "of formal"
for parameters); declaring a name in a function that also exists in the root
table is allowed (shadowing).

References are checked in two passes:

- **Pass 1, while walking in source order**: every `lvalue` (the target of
  an assignment and every variable read, `a`, `a.b`, `a[e]`) must already be
  declared, in the current table or any ancestor, as a *variable*:
  "Variable 'x' is referenced but not defined" otherwise, "Identifier 'x' is
  not a variable" if it names a function. Consequence: a function body that
  uses a top-level variable declared **later in the file** fails, and so does
  a use before the `var` inside the same body [S][P].
- **Pass 2, after all declarations are known**: every call `f(args)` and
  every `new C` must name a *function* (script function, method, native,
  external) anywhere in scope, declared before or after; "Function 'f' is
  referenced but not defined", "Trying to use variable f as a function",
  "Object 'C' is referenced but no constuctor is defined" (sic), "Trying to
  use variable C as an object". Every method-call receiver `o.M()` /
  `o[e]()` must name a *variable* (any variable declared anywhere visible,
  including later top-level ones): "Method is called for 'o' which is not
  defined", "Trying to call method of a non-variable o".

The only effect on the output: a call `f(...)` or `new C(...)` compiles to
`EXTERNAL_CALL` if the resolved symbol is a **native or `external`** function
(the `external` flag), and to `CALL` if it is a function or method defined in
this script.

**Weed checks** (before the symbol passes) [S]:

- "Invalid break": `break` outside a loop and outside a `switch`;
- "Invalid continue": `continue` outside a loop (a `switch` alone does not
  count);
- "Event handler cannot return a value": `return exp;` inside an `on` body;
- a declaration with several names is split into one declaration per name,
  each with the same type and the *same* initializer expression (see below
  for the effect on line numbers).

The two checks on `break`/`continue` are purely syntactic, not related to
the broken label resolution described in [Loops](#loops-switch-break-and-continue).

### Natives

The compiler needs the names the engine provides. The WME 1.x engine
publishes (`CBGame::PublishNatives`, `CAdGame::PublishNatives`) [S]:

- functions (compile to `EXTERNAL_CALL`): `LOG String MemBuffer File Date
  Array TcpClient Object Sleep WaitFor Random SetScriptTimeSlice MakeRGBA
  MakeRGB MakeHSL RGB GetRValue GetGValue GetBValue GetAValue GetHValue
  GetSValue GetLValue Debug ToString ToInt ToBool ToFloat Actor Entity`
  (the names `String`, `File`, `Date`, `Array`, `Object`, `MemBuffer`,
  `TcpClient`, `Actor`, `Entity` are the "classes" used with `new`);
  plugin classes registered by loaded plugins are added to this list;
- variables (valid receivers and lvalues): `Game Math Directory self this`.

Everything else a script uses (`Scene`, `Keyboard`, `actor`, ...) must be
declared by the script itself (the games `global`-declare them in an include
such as `base.inc`). The list is a parameter of the Luma compiler
(`luma_compiler::Natives`; `Natives::wme()` is the list above, `luma
compile --native <name>` adds a plugin class); calling any other name that
no script declares is a compile error, as in the original.

## Output layout

All integers are little-endian `u32` unless stated [S][V]. Offsets are from
the start of the file. The file is the concatenation, **with no padding and
no alignment**:

```
offset  size   content
0       4      magic 0xDEC0ADDE                      (bytes DE AD C0 DE)
4       4      version 0x0102
8       4      offset of the code        = 32 + len(name) + 1
12      4      offset of the function table
16      4      offset of the symbol table  (the original calls it "string table")
20      4      offset of the event table
24      4      offset of the externals table
28      4      offset of the method table
32      n+1    the script name as given to the compiler, NUL-terminated
..             code
..             function table
..             symbol table
..             event table
..             externals table
..             method table                           (end of file)
```

The five table offsets are patched in as each table starts; each table
begins exactly where the previous one ends, and the file ends with the last
byte of the method table. The header is always the version-`0x0102` one:
the externals table and the method table are always present, even when
empty (an empty table is a single `u32` zero). The compiler always writes
`SCRIPT_VERSION` 0x0102, regardless of the features used.

**The name** written at offset 32 is the file name the host passed to the
compiler. The editor's packager passes the project-relative path with
backslashes (`scripts\scene.script`; this is what all 352 compiled scripts
of the two test games contain, case preserved from the file name); the
command-line tool passes whatever path it was given. When the engine loads
a script from a package with forward slashes, `BPkgFile::Open` rewrites the
name in place to backslashes before the compiler sees it, so a source name
reaching the compiler through that path is backslash-separated [S][V].

**Function table.** `count`, then for each *non-method* function:
`position: u32`, `name: NUL-terminated`. `position` is the file offset of
the function's first instruction (its `SCOPE`), not of the `JMP` in front
of it.

**Symbol table.** `count`, then for each symbol, in index order:
`index: u32` (equal to its position in the list), `name: NUL-terminated`.

**Event table.** Same shape as the function table: `count`, then
`position: u32`, `name` (the string of `on "name"`, case preserved).

**Externals table.** `count`, then for each `external` declaration:
`dll: string`, `name: string`, `call_type: u32` (0 stdcall, 1 cdecl,
2 thiscall), `return_type: u32`, `num_params: u32`, then `num_params` times
`param_type: u32`. Type codes: 0 void, 1 bool, 2 long, 3 byte, 4 string,
5 float, 6 double, 7 membuffer (`short` and `int` are written as 2) [S][D].

**Method table.** Same shape as the function table, for functions declared
with `method`.

### Order of the table entries [S][V]

The function, event, method and externals entries are written in the
**reverse of source order**: the compiler prepends each new entry to a
linked list and writes the list from its head. Verified on the 152 scripts
of The White Chamber and 48 of OpenQuest that have several events: the event
positions are strictly decreasing. Consequence for the engine: it searches
events from the end of the table, so with two handlers of the same name the
**first** one in the source wins (`CScScript::GetEventPos`); a compiler
that wrote the tables in source order would change which duplicate handler
runs. (`luma_script::asm::Assembler` writes tables in declaration order; it
is a test tool and does not need to change, but a compiler cannot reuse its
table writer as is.)

The symbol table is in **first-use order**: indexes are assigned the first
time a name is written as an operand during code generation, so the order
depends on the exact emission order, including the order in which argument
expressions are generated (right to left) and in which the two sides of an
assignment are generated (value first, target last).

### What goes into the symbol table [S][V]

Only the following operands are symbols: the variable name of `DEF_VAR`,
`DEF_GLOB_VAR`, `DEF_CONST_VAR`, `PUSH_VAR`, `PUSH_VAR_REF`, `POP_VAR`,
`PUSH_THIS`, and the function name of `EXTERNAL_CALL`. A name that is
declared but never reaches one of these (an unused `external`, a function
that is never called with `EXTERNAL_CALL`) is **not** in the table: there are
no "unused symbols" and no declared-but-unreferenced entries. Script
function names, method names, event names and member names are not symbols
(function names appear in the function/method tables; member and method
names appear as inline `PUSH_STRING` operands). Names are compared exactly
(byte equality, case-sensitive).

**Strings are not tabulated.** Each `PUSH_STRING` carries its bytes inline,
terminated by NUL, even if the same text occurs many times. Strings are
written as they are after escape processing; there is no encoding step.

## Code generation

### Writing primitives [S]

Let `ip` be the current output offset.

- `emit(line, OP)`: if `line > 0` and `line != cur_line`, first write
  `DBG_LINE` and a `u32` operand `line`, and set `cur_line = line`; then
  write the opcode as a `u32`. `cur_line` starts at `-1`, is **never reset**
  (not at function boundaries either) and is only changed by `emit`. A line
  of `-1` never produces a `DBG_LINE`.
- Operands are then written with plain `put32`, `put_f64` (8 bytes,
  little-endian IEEE 754) or `put_str` (bytes plus NUL), without any
  `DBG_LINE` logic.
- `sym(name)`: index of `name` in the symbol list, appending it on first use.
- Forward jumps: write `0`, remember the operand's offset, patch with the
  target offset later. Targets are absolute file offsets.

`line` for a node is its recorded `lineno`; how those are computed is the
subject of [Line numbers](#line-numbers). Notation below: `ln(X)` is the
recorded line of node `X`.

### File body [S][V]

```
header (32 bytes), name
cur_line = -1
(1) every top-level statement run, in source order, generated with stmt()
(2) every function and method, in source order, generated with function()
(3) every event handler, in source order, generated with event()
(4) every external declaration (no code; only queues a table entry)
emit(ln(script), RET)
resolve CALL targets, resolve break/continue targets
write the tables, patch the header offsets
```

Top-level code therefore comes first regardless of where the statements are
in the source, then all function and method bodies, then all events; each
body is wrapped in a `JMP` so that the top-level code falls through to the
single final `RET`. `ln(script)` is the line counter value at the end of
the input (1 + the number of newlines of the main file) [V].

The top-level code is **not** preceded by a `SCOPE` (the original has that
instruction commented out), so a top-level `var` defines a script-level
variable and top-level `global`/`const` define engine-wide ones. `SCOPE` is
emitted only at the start of each function, method and event body; locals of
a body are the `DEF_VAR`s executed inside it, whatever the block nesting in
the source. `PUSH_VAR_THIS` is never emitted. [S][V]

### Functions, methods, events [S][V]

```
function F(p1, ..., pn) body:
    emit(ln(F), JMP)           ; operand patched to the end of the function
    (the position stored in the table is the next offset)
    emit(ln(F), SCOPE)
    emit(ln(F), CORRECT_STACK) ; operand n (number of parameters)
    for each parameter p in order:
        emit(ln(p.type), DEF_VAR)  sym(p)
        emit(ln(p), POP_VAR)       sym(p)
    stmt(body, in_event = false)
    emit(ln(F), PUSH_NULL)
    emit(ln(F), RET)
    (patch the JMP operand to the next offset)

event "E" stm:
    emit(ln(E), JMP)           ; operand patched to the end
    (position recorded here, then:)
    emit(ln(E), SCOPE)
    stmt(stm, in_event = true)
    emit(ln(E), RET_EVENT)
    (patch the JMP)
```

`ln(F)` and `ln(E)` are the line of the **closing brace** of the body for
the normal case of a block body (see [Line numbers](#line-numbers)), so a
function starts with a `DBG_LINE` of its last line and then its first
statements bring the counter back [V]. A method is generated exactly like a
function; only the table differs. Parameters are popped in declaration order
because the caller pushes arguments last-to-first. `CORRECT_STACK n` makes
the callee tolerate a wrong argument count at run time.

Within an event handler `return;` is `RET_EVENT`; within functions, methods
and top-level code, `return;` is `PUSH_NULL; RET` and `return e;` is `e; RET`
[S].

### Statements [S][V unless marked]

`stmt` below; `exp(e, value)` is the expression generator described in the
next section (the `value` flag is false only for an expression statement).

| Statement | Sequence |
|-----------|----------|
| `;` | nothing |
| expression statement `e;` | `exp(e, false)` (only calls and `new` then drop their result) |
| `return;` | in an event: `emit(ln, RET_EVENT)`; otherwise `emit(ln, PUSH_NULL)`, `emit(ln, RET)` |
| `return e;` | `exp(e, true)`, then `emit(ln, RET)`; in an event, `RET_EVENT` only (the value is rejected by weed, so this does not occur) |
| `if (c) S` | `exp(c, true)`; `emit(ln(if), JMP_FALSE)` + operand (patched to the offset after `S`); `S` |
| `if (c) S1 else S2` | `exp(c, true)`; `emit(ln(if), JMP_FALSE)` + operand A; `S1`; `emit(ln(if), JMP)` + operand B; patch A to here; `S2`; patch B to here |
| `while (c) S` | `L0 = ip`; `exp(c, true)`; `emit(-1, JMP_FALSE)` + operand E; `S`; `emit(ln(while), JMP)` operand `L0`; patch E to here |
| `for (I; c; U) S` | inits (below); `L0 = ip`; `exp(c, true)`; `emit(-1, JMP_FALSE)` + operand E; `S`; `L1 = ip`; `exp(U, true)`; `emit(ln(for), JMP)` operand `L0`; patch E to here |
| `switch (e) {...}` | see below |
| `break;` | `emit(ln, JMP)` + operand 0 (queued for resolution) |
| `continue;` | `emit(ln, JMP)` + operand 0 (queued for resolution) |
| block `{ ... }` | its statements in order, nothing else |
| declaration | see below |

`for` initializers: each item in order; a `var x [= e]` item is a
declaration (below), an expression item is `exp(e, true)` (so a call there
leaves its result on the stack). The **update list** is generated with
`exp(U, true)` for each item in source order: a call in an update leaves
its return value on the stack, never popped. An `else`-less `if` and the
`then`/`else` parts are each wrapped in an (empty-effect) scope node by the
parser; the code generator ignores scope nodes: **no `SCOPE` instruction
is generated for blocks**, only for functions and events.

`JMP_FALSE` and `JMP` in `while`/`for` conditions use line `-1` for the
conditional jump (no `DBG_LINE`); the back-jump uses the statement's line.

**Switch.** [S][V]

```
loop_nest++, loop_level++, this switch's id = loop_nest
S0 = ip
exp(e, true)
emit(ln(switch), POP_REG1)
pending_end = none
for each case in order (the default, if any, is last):
    normal case with label X and body B:
        exp(X, true)
        emit(ln(case), PUSH_REG1)
        emit(ln(case), CMP_EQ)
        emit(ln(case), JMP_FALSE) + operand N   ; next test
        if pending_end: patch it to here
        B
        emit(ln(case), JMP) + operand pending_end ; patched later: see the note below
        patch N to here                            ; next case test starts here
    default with body B:
        if pending_end: patch it to here
        B
record (id, level) -> (start = S0, end = ip)       ; for break/continue
if pending_end: patch it to here
loop_level--
```

Notes on the `switch` code: the `JMP` after each case body goes to
`pending_end`, which is **patched to the start of the next case's body, not
to the end of the switch** (fall-through): after the `JMP` at the end of case
1, it lands at the first instruction of case 2's body (after case 2's
test). In other words a case body that does not end in `break;` runs into the
next body; and the very last `JMP` is patched to the end of the switch. The
register `POP_REG1`/`PUSH_REG1` holds the switch value for the tests. The
`continue` statement inside a `switch` targets the switch's *start* `S0`
(re-evaluating the switch expression), not the enclosing loop [D].

### Loops, switch, `break` and `continue` (the label bug) [S][V for `break`]

The generator keeps two global counters, `loop_nest` (incremented for every
`while`, `for` and `switch` *entered*, never decremented) and `loop_level`
(incremented on entry, decremented on exit). A `break`/`continue` records
`(loop_nest, loop_level)` **as they are at that moment** and each loop,
when finished, records `(its own nest id, its level) -> (start, end)`. After
the whole file is generated, each queued jump is resolved by finding the
loop with the same pair; if there is none, its operand stays **0**.

For `while` the recorded pair is `(id, level)` with `start = L0` and
`end` = the offset after the loop; for `for`, `start = L1` (the update
section, so `continue` runs the update) and `end` = after the loop; for
`switch`, `start = S0` and `end` = after the switch.

The consequence is a faithful bug that must be reproduced: because
`loop_nest` is never decremented, a `break`/`continue` that comes after a
nested loop **or `switch`** inside an outer loop has a `loop_nest` greater
than the outer loop's id and matches nothing, so it is written as `JMP 0`
(a jump to the file header). For example [D, derived and run through the
port]:

```
while (a) {           ; id 1, level 1
    while (b) { }     ; id 2, level 2, finished
    if (c) break;     ; recorded (2, 1): no loop has that pair -> JMP 0
}
while (true) {        ; id 1, level 1
    switch (x) { case 1: Sleep(1); break; }   ; id 2, level 2: its break is fine
    continue;         ; recorded (2, 1) -> JMP 0
}
```

A `break` directly in a loop body before any nested loop, or inside a
`switch`/loop for which it is the innermost enclosing one and that has no
nested loop before it (the usual `break;` ending a `case`), is fine. A
`break` in a loop that follows a *previous sibling* loop is also fine,
because ids are unique. The weed check ("Invalid break") is independent of
this. None of the 1393 `break` statements of the corpus hits the bug (no
compiled script of either test game contains a `JMP` or `JMP_FALSE` with
operand 0).

### Declarations [S][V]

For each name of a declaration, in order (a multi-name declaration was split
into one declaration per name by weed, each with the same initializer
expression):

```
emit(ln(type), DEF_VAR | DEF_GLOB_VAR | DEF_CONST_VAR)   operand sym(name)
if there is an initializer:
    exp(init, true)
    emit(ln(decl), POP_VAR)  operand sym(name)
```

`var` is `DEF_VAR`, `global` is `DEF_GLOB_VAR`, `const` is `DEF_CONST_VAR`.
Without an initializer there is nothing after the `DEF_*` (the engine
defines the value as `null`). `ln(type)` is the line of the keyword token;
`ln(decl)` is the line of the `;`, with two exceptions: the second and later
names of a multi-name declaration use the line **at the end of the input**
(the split declarations are created after parsing, with the final value of
the line counter) [D], and a `for (var x ...)` initializer uses the line of
the token that follows the initializer (see below). A multi-name
declaration with an initializer evaluates the initializer expression once
**per name** (code is generated for each) [D]: `var a, b = f();` calls `f`
twice.

### Expressions [S][V unless marked]

`exp(e, value)`; `value` is false only for the top of an expression
statement. `ln(e)` is the line recorded in `e`. Emission is by kind. For
every node the instructions are emitted with `ln(e)` unless noted.

| Expression | Sequence |
|------------|----------|
| integer `n` | `PUSH_INT` operand `n` as a two's complement `u32` |
| `true` / `false` | `PUSH_BOOL` operand 1 / 0 |
| float `x` | `PUSH_FLOAT` operand `x` as an 8-byte `f64` [D] |
| string `s` | `PUSH_STRING` operand `s` NUL-terminated |
| `null` | `PUSH_NULL` |
| `-x` | `PUSH_INT 0`, `exp(x)`, `SUB`  (there are no negative literals) |
| `!x` | `exp(x)`, `NOT` |
| `(var) x` | **nothing at all**: the generator has no case for a cast, so neither the cast nor its operand produces code (`var a = (var) 5;` is `DEF_VAR a; POP_VAR a`, and the pop finds an empty stack at run time) [D, confirmed on the port] |
| variable `v` | `PUSH_VAR sym(v)` |
| `o.m` (read) | `PUSH_VAR_REF sym(o)`, `PUSH_STRING "m"`, `PUSH_BY_EXP` |
| `o[x]` (read) | `PUSH_VAR_REF sym(o)`, `exp(x)`, `PUSH_BY_EXP` |
| `a = b` (target `v`) | `exp(b)`, `POP_VAR sym(v)` (with the **lvalue's** line) |
| `a = b` (target `o.m`) | `exp(b)`, `PUSH_VAR_REF sym(o)`, `PUSH_STRING "m"`, `POP_BY_EXP` (lvalue's line) |
| `a = b` (target `o[x]`) | `exp(b)`, `PUSH_VAR_REF sym(o)`, `exp(x)`, `POP_BY_EXP` (lvalue's line) |
| `l OP r` | `exp(l)`, `exp(r)`, then the operator instruction |
| call `f(a1,...,an)` | see below |
| `new C(a1,...,an)` | see below |
| `o.m(a1,...,an)` / `o[x](a1,...,an)` | see below |

Note that an assignment leaves nothing on the stack (it ends with `POP_VAR`
or `POP_BY_EXP`). The code of `x = y = 1` is therefore `PUSH_INT 1; POP_VAR
y; POP_VAR x`, and the second pop finds an empty stack at run time; likewise
`if (a = b)` leaves no value for the `JMP_FALSE`. The compiler accepts both
and emits exactly that; chained assignment and assignment in a condition are
effectively unusable [D; the corpus only has the plain `x = e;` form and
`for` update assignments].

Operator instructions: `+` `ADD`, `-` `SUB`, `*` `MUL`, `/` `DIV`, `%`
`MODULO`, `&&` `AND`, `||` `OR`, `==` `CMP_EQ`, `!=` `CMP_NE`, `<` `CMP_L`,
`>` `CMP_G`, `<=` `CMP_LE`, `>=` `CMP_GE`, `===` `CMP_STRICT_EQ`, `!==`
`CMP_STRICT_NE`. Both operands of `&&` and `||` are always evaluated (no
short circuit; `AND`/`OR` are ordinary binary instructions). All of them use
the binary node's line.

**Function call `f(a1..an)`** [S][V]:

```
exp(an), exp(a(n-1)), ..., exp(a1)      ; arguments pushed RIGHT TO LEFT
PUSH_INT n
EXTERNAL_CALL sym(f)       ; if f is a native / external
 or  CALL <address of f>   ; if f is a function or method of this script,
                           ;   the operand is written as 0 and patched at the end
if (!value) POP_EMPTY     ; the call's result is dropped
```

Argument evaluation order is right to left, which determines the symbol
table order too. A `CALL` target is looked up by name among the script's
functions *and methods* (the table is searched from the most recently
defined; duplicates cannot occur).

**Method call `o.m(a1..an)`** (the name is the string `"m"`; `o[x](...)`
uses `exp(x)` instead of the `PUSH_STRING`) [S][V]:

```
PUSH_THIS sym(o)
exp(an), ..., exp(a1)
PUSH_INT n
PUSH_VAR sym(o)
PUSH_STRING "m"            ; or exp(x)
CALL_BY_EXP
if (!value) POP_EMPTY
POP_THIS
```

The `POP_EMPTY` for a dropped result comes **before** `POP_THIS`.

**`new C(a1..an)`** (arguments optional) [D]:

```
CREATE_OBJECT
PUSH_THIS_FROM_STACK
exp(an), ..., exp(a1)
PUSH_INT n
EXTERNAL_CALL sym(C)    or    CALL <address of C>
POP_THIS
POP_EMPTY               ; the constructor's result
if (!value) POP_EMPTY   ; the new object, when the expression is a statement
```

With `value` true the new object stays on the stack.

**Argument and expression lists** are linked through the node's `next`
pointer, and each node generated re-enters the generator for its `next`
with `value = true`; `swapArguments` reversed argument lists at parse
time, which is where "right to left" comes from. List positions that do not
go through `swapArguments` keep source order: the `for` initializer list
and the `for` update list, both left to right [S][V].

### A complete example [P]

Source (name `demo.script`, so the code starts at 32 + 11 + 1 = 44; LF line
ends):

```
1: global G = 1;
2: var x = 2;
3: function add(a, b)
4: {
5:   return a + b;
6: }
7: on "LeftClick"
8: {
9:   x = add(1, 2);
10:   Game.Msg("hi");
11: }
```

Output, as a listing (`@offset`); the file is 450 bytes:

```
header: magic, 0x0102, code=44, functions=375, symbols=387, events=424,
        externals=442, methods=446; name "demo.script\0" at 32
@44  DBG_LINE 1; DEF_GLOB_VAR G; PUSH_INT 1; POP_VAR G
@76  DBG_LINE 2; DEF_VAR x; PUSH_INT 2; POP_VAR x
@108 DBG_LINE 6; JMP 224                      ; over the function; line 6 is its closing brace
@124 SCOPE; CORRECT_STACK 2                   ; position of "add" = 124
@136 DBG_LINE 3; DEF_VAR a; POP_VAR a; DEF_VAR b; POP_VAR b
@176 DBG_LINE 5; PUSH_VAR a; PUSH_VAR b; ADD; RET
@208 DBG_LINE 6; PUSH_NULL; RET               ; implicit return
@224 DBG_LINE 11; JMP 363                     ; over the handler; line 11 is its closing brace
@240 SCOPE                                    ; position of "LeftClick" = 240
@244 DBG_LINE 9; PUSH_INT 2; PUSH_INT 1; PUSH_INT 2; CALL 124; POP_VAR x
@292 DBG_LINE 10; PUSH_THIS Game; PUSH_STRING "hi"; PUSH_INT 1; PUSH_VAR Game;
                  PUSH_STRING "Msg"; CALL_BY_EXP; POP_EMPTY; POP_THIS
@351 DBG_LINE 11; RET_EVENT
@363 DBG_LINE 12; RET                         ; line 12 = end of input
@375 functions:  1, (124, "add")
@387 symbols:    5, (0,"G") (1,"x") (2,"a") (3,"b") (4,"Game")
@424 events:     1, (240, "LeftClick")
@442 externals:  0
@446 methods:    0                            ; file ends at 450
```

(`PUSH_INT 2; PUSH_INT 1; PUSH_INT 2; CALL` is the call `add(1, 2)`: the
arguments right to left, then the count.)

## Line numbers

`DBG_LINE` is the part most likely to differ in a reimplementation, because
the line recorded in a node is **not** the line where the construct starts.
Every `make...` constructor in `tree.c` stores the global `lineno` at the
instant the Bison action runs, i.e. when the production is **reduced**,
and the scanner's counter at that instant is the line of the most recent
token the parser has fetched. Bison fetches a lookahead token only when the
automaton state cannot decide without it; states whose only action is a
single reduction (a *default reduction*) reduce without fetching [S].
Therefore:

> The line of a node is the line of the last token **consumed** by its
> production if the production was reduced without a lookahead fetch, and the
> line of the **next token after the production** if the lookahead had been
> fetched. A lookahead that has been fetched stays fetched until the next
> shift, so a production reduced right after a "late" one inherits the
> later line.

Two ways to reproduce this exactly:

1. Use the original automaton. `parse.c` contains the Bison 1.24 tables
   (`yytranslate`, `yyr1`, `yyr2`, `yydefact`, `yydefgoto`, `yypact`,
   `yypgoto`, `yytable`, `yycheck`, at lines 131, 261, 277, 293, 319, 326,
   352, 363 and 435 of `parse.c`; constants `YYFLAG = -32768`,
   `YYNTBASE = 71`, `YYLAST = 684`, `YYFINAL = 222`; token numbers in
   `parse.h`, 258 and up for named tokens, the ASCII code for single
   characters; the semantic action of rule *n* is `case n:` in the big
   switch of `parse.c` and the matching action in `lang.y`).
   The driver loop is the one in `bison.simple`: `n = yypact[state]; if n ==
   YYFLAG then reduce by yydefact[state] without reading a token; else read
   a token if none is pending, look up yytable[n + token] guarded by yycheck;
   shift (discard the pending token) or reduce`. After a reduction, the goto
   is `yypgoto[lhs - YYNTBASE] + top` checked against `yycheck`, else
   `yydefgoto`. A Rust port that embeds those tables (data only; keep the MIT
   notice and list the file in `NOTICE` per `CLAUDE.md`) and records the
   scanner line in every semantic action reproduces the lines exactly [V].
   The port used to validate this document is that.
2. Write a recursive-descent parser and emulate the fetch timing with the
   table below. The table is derived mechanically from the same Bison
   tables. **This is the route `luma-compiler` takes** (no Bison tables in
   the repository): its parser fetches the next token only where the
   automaton does and reads the scanner's counter when it builds a node;
   `crates/luma-compiler/src/parser.rs` names the production and the case
   at each node. Checked against route 1 on the whole corpus (288 of 288)
   and on snippets of the [D] constructs with blanks turned into newlines
   at random, so that every production meets a line change [V].

**Lateness of the productions that create line-bearing nodes** (from the
`yypact`/`yydefact` tables [S]; "late" means the reduction state needs the
lookahead, so the lookahead is fetched; "early" means it reduces without
fetching, so the line is the last token's unless an earlier reduction in the
same chain already fetched). The rule behind the table: a production is
late when the state it is reduced in can still shift something (an
operator after an expression, `else` after an `if`, `(` after `new C` or
after an lvalue, another statement after a `case` body), and early when
that state has nothing but the reduction; Bison also removes the shifts
that operator precedence resolves in favour of the reduction, which is why
`%`, the tightest operator, is early while `*` and `/` (which can still
shift a `%`) are late.

| Node | Rules | Line recorded |
|------|-------|---------------|
| integer, boolean, float, string constants, `null` | 98, 99, 100, 101, 90 | that token's line |
| variable read / lvalue (`a`, `a.b`, `a[e]`) | 123, 124, 125 (late), 102 | the line of the **next token** after the lvalue |
| call `f(...)`, method call `o.m(...)`, `o[x](...)`, `new C(...)` | 104, 105, 106, 108 (early) | the line of the closing `)` |
| `new C` without parentheses | 107 (late) | the line of the next token |
| binary `+ - * / == != === !== < > <= >= && \|\|` | 75-86, 88, 89 (late) | the line of the **next token after the right operand** |
| binary `%` | 87 (early) | inherited from the right operand |
| unary `-`, `!`, cast | 92, 93, 96 (early) | inherited from the operand |
| assignment | 74 (late) | the next token after the right-hand side (the line used for `POP_VAR`/`POP_BY_EXP` is the **lvalue's**, i.e. the token after the target, since 123-125 are late) |
| parenthesised expression | 103 | the inner node keeps its own line; the parentheses create no node |
| `return` | 56, 57 (early) | the `;` |
| `if` without `else` | 58 (late) | the line of the **first token after the whole statement** (after the body) |
| `if ... else` | 59 (early) | inherited from the `else` statement |
| `while` | 60 (early) | inherited from the body |
| `for` | 61 (early) | inherited from the body |
| `break;`, `continue;` | 65, 66 (early) | the token's line |
| `switch` | 67 (early) | the closing `}` |
| `case` | 117, 118 (late) | the line of the next token after the case body (the next `case`, `default` or `}`) |
| declaration `var x ...;` | 10 (early) | the `;` |
| the type of a declaration | 16, 17, 18 (early) | the keyword token |
| `for` initializer `var x [= e]` | 11 | the next token after the initializer (or after the name) |
| parameter | 50 (early) | the parameter name token |
| function / method | 43, 44 (early) | inherited from the body: the closing `}` |
| event | 45 (early) | inherited from the statement: the closing `}` of a block body |
| the script (final `RET`) | 1 | the line at the end of the input |
| the extra declarations made by splitting `var a, b` | (weed) | the line at the end of the input |

Rules whose nodes carry no line used by the code generator (identifiers,
empty lists, toplevel wrappers, `exps`, `neexps`, `dcases`, ...) are
omitted. The "next token" for the last token of the file is the end of
input, whose line is the final counter value. Inside an included file the
counter is frozen at the line of the `#include`, so "the last token" and
"the next token" of anything inside the include have that line.

**Worked example** (source lines are numbered; the listing shows the
`DBG_LINE` values the generator emits, as produced by the port) [P]:

```
1: var a = 1 +
2:    2
3:    ;
4: var x;
5: if (a == 3)
6:   a = 4;
7:
8:
9: // comment
10: var y;
11: Sleep(a,
12:   a
13: );
```

```
DBG_LINE 1; DEF_VAR a; PUSH_INT 1;
DBG_LINE 2; PUSH_INT 2;
DBG_LINE 3; ADD              ; '+' is late: line of the next token, ';' on line 3
            POP_VAR a        ; the declaration's line is the ';' too (3): no new DBG_LINE
DBG_LINE 4; DEF_VAR x
DBG_LINE 5; PUSH_VAR a; PUSH_INT 3; CMP_EQ     ; (a, 3, == all on line 5)
DBG_LINE 10; JMP_FALSE       ; 'if' is late: the first token after the statement is 'var' on line 10
DBG_LINE 6; PUSH_INT 4; POP_VAR a
DBG_LINE 10; DEF_VAR y
DBG_LINE 13; PUSH_VAR a      ; the last argument 'a' (read): next token is ')' on line 13
DBG_LINE 11; PUSH_VAR a      ; the first argument 'a': next token is ',' on line 11
DBG_LINE 13; PUSH_INT 2; EXTERNAL_CALL Sleep; POP_EMPTY
DBG_LINE 14; RET             ; the file ends with a newline after line 13: counter is 14
```

(`CMP_EQ` shows no `DBG_LINE`: the whole condition is on line 5 and `)` is
the next token.)

## Byte-identity checklist

Things that decide whether the output is identical, in the order a
reimplementation is most likely to get them wrong:

1. **Line numbers** (above): node lines come from the parser's lookahead
   timing, not from the source position of the construct; included files
   freeze the counter; the final value of the counter is used for the last
   `RET` and for split declarations.
2. **Table order and symbol order**: function, event, method and external
   entries in reverse source order; symbols in first-use order, which
   depends on the exact generation order (arguments right to left, value
   before target in assignments, `PUSH_THIS name` before the arguments of a
   method call).
3. **Emission order**: all top-level statements first, then functions and
   methods in source order, then events, then the single final `RET`; a
   `JMP` over each body; no `SCOPE` for blocks.
4. **Loop labels**: `break`/`continue` resolve by `(nest, level)` pairs,
   producing `JMP 0` in the cases described, `continue` in a `for` goes to
   the update, `continue` in a `switch` goes to the switch start, and the
   conditional jump of `while`/`for` carries no `DBG_LINE`.
5. **Call classification**: `EXTERNAL_CALL` iff the callee is a native or
   `external`; this depends on the native list (see [Natives](#natives)).
6. **Parser shape**: operator precedence (the unusual levels), `case` labels
   limited to postfix expressions, `null` only at `exp` level, one-level
   member access, `break;`/`continue;` as single tokens.
7. **Literals**: integers are `atoi` of unsigned decimal text and negative
   numbers are `0 - n` code; floats are `atof` stored as an IEEE `double`;
   strings carry bytes unchanged after the three `~` escapes.
8. **Header and tail**: version always `0x0102`; code offset is
   `32 + len(name) + 1`; no padding; the name written is the project
   relative path with backslashes.

## Diagnostics

For reference, the messages of the original (the line is the counter at the
moment of the error):

| Phase | Message |
|-------|---------|
| scanner | `Unclosed comment.`, `Includes nested too deeply (20)`, `Error opening include file <name>`, `Include filename expected`, `Unterminated string constant`, `String constant too long`, `Invalid character` |
| parser | `syntax error` |
| weed | `Invalid break`, `Invalid continue`, `Event handler cannot return a value` |
| symbols, pass 1 | `Duplicate declaration of function '<f>'`, `Duplicate declaration of formal '<x>'`, `Duplicate declaration of variable '<x>'`, `Variable '<x>' is referenced but not defined`, `Identifier '<x>' is not a variable` |
| symbols, pass 2 | `Function '<f>' is referenced but not defined`, `Trying to use variable <f> as a function`, `Object '<C>' is referenced but no constuctor is defined`, `Trying to use variable <C> as an object`, `Method is called for '<o>' which is not defined`, `Trying to call method of a non-variable <o>` |

After a scanner error the scanner reports end of input, so a "syntax error"
usually follows; only the first matters. Limits of the original that a
reimplementation need not copy but may meet in practice: string constants
longer than 4096 characters; the right-recursive lists (top-level items,
`var` name lists, argument lists, parameter lists, `case` lists) grow the
Bison parse stack, which is capped at 10000 entries ("parser stack
overflow"), i.e. a script with several thousand top-level items fails.

## WME Lite and WME 1.x

- The WME Lite repository (MIT, `src/ScEngine.cpp`, `src/dcscript.h`,
  `src/BGame.cpp`, `src/AdGame.cpp`) contains **no compiler**. Its engine
  loads the same `dcscomp.dll` through the same six entry points
  (`CompileBuffer`, `CompileFile`, `ReleaseBuffer`, `SetCallbacks`,
  `DefineFunction`, `DefineVariable`) on Windows, and has no compiler at all
  elsewhere (`m_CompilerAvailable = false`: an uncompiled script fails to
  load with "needs to be compiled but compiler is not available").
- `dcscript.h` is identical in the two (opcode list, `SCRIPT_MAGIC`,
  `SCRIPT_VERSION 0x0102`), as are the lists of natives published by
  `PublishNatives` for the base and the adventure game.
- In WME 1.x the host side is: the editors' packager
  (`ProjectMan/FilterScript.cpp`) compiles every `*.dcs` and `*.script` file
  of a project to the package with the project-relative path as name;
  `wme_comp.exe` compiles one file given on the command line. Neither does
  anything to the output.

So there is no visible difference in the format or in the compiler's
behaviour between the two engines; Luma Engine, which already reads both
generations of games, needs only this one specification.

## Open questions

1. **Integer literals above `INT_MAX`.** The text is converted with `atoi`;
   MSVC's `atoi` is believed to saturate at 2147483647 but this was not
   checked against a binary. Scripts with such constants are unlikely.
2. **Float literals with many digits.** `atof` of the MSVC runtime of the
   era was not always correctly rounded for inputs beyond 15-17 significant
   digits; Rust's `str::parse::<f64>` is. Short literals (the practical
   case) agree.
3. **The shipped `dcscomp.dll`.** This document describes the source
   tree's compiler; the White Chamber corpus (276 scripts) matched it
   exactly, which supports that the shipped build is this compiler.
4. **Natives list in other builds.** Games compiled with a different
   native list (plugins, later engine versions adding functions) would
   classify calls differently only if a name was a native in one build and
   a script function in the other; this does not occur in the corpus.
5. **Constructs not in the corpus** are marked [D]: `continue`, `new`,
   floats, `*`, `%`, `===`/`!==`, `const`, `external` and its table,
   initialised multi-name declarations (their line), the `JMP 0` case.
   `luma-compiler` agrees with the port of the original's source on all
   of them; compiled scripts of other games containing them would settle
   whether the shipped binary does too.
6. **Scanner corner cases.** Embedded NUL bytes, a lone `\r`, bytes in
   identifiers, and the 20th include level (an array overrun in the
   original) were not exercised.
7. **Empty `for` initializer.** Crashes the original; `luma-compiler`
   rejects it with an error.
8. **Behaviour of the engine for the data the compiler writes** (for
   example how `DEF_GLOB_VAR x = e;` initialises an existing global) is the
   virtual machine's concern and is documented with it.

## Implementation in Luma

The crate `crates/luma-compiler` (a core crate; it depends on
`luma-script` for the opcode numbers and the header constants, and on
nothing else) follows the pipeline above, one module per phase:

| Module | Phase | Original source it is written against |
|--------|-------|---------------------------------------|
| `lexer.rs` | tokens, `#include` expansion, the frozen line counter | `lang.l`, `main.c` |
| `parser.rs` | recursive descent to a syntax tree with recorded lines | `lang.y`, `tree.c`, `parse.output` (for the lateness) |
| `symbols.rs` | the weed checks and the two symbol passes | `weed.c`, `symbol.c` |
| `codegen.rs` | code generation, back-patching, the tables | `opcode.c` |

Its public surface: `Compiler::new(natives).compile(source, name,
&mut includes)` returns the file's bytes or the first `Error`; `Natives`
is the list of [Natives](#natives) (`Natives::wme()` by default, plus
`.function(name)` for plugin classes); `Includes` is the include loader
(any `FnMut(&[u8]) -> Option<Vec<u8>>`, or `GameDirs`, which resolves a
name with `\` or `/` against one or more directories without regard to
case, like the engine's file manager). `luma compile <source> [-o out]
[--game dir] [--native name]...` wraps it: the `--game` directory (or
the source's own directory) is the include root, and the name written in
the header is the source's path relative to it with backslashes.

**Errors** name the file, line and column where the offending token is:
`scripts\base.inc:3:8: Duplicate declaration of variable 'G'`. Those are
real positions in the file the token comes from (an include is named as
written in its directive), unlike the line the original reports, which is
its frozen counter. The messages are the original's where it has one
([Diagnostics](#diagnostics)); syntax errors add what was expected and
found.

**Deliberate deviations**, all in cases where the original has no
output:

- an empty `for` initializer list is an error ("for loop without
  initializer"), where the original crashes;
- a call inside the method expression of `o[e](...)` or inside the member
  of an assignment target `a[e] = v` is resolved like any other call,
  where the original leaves it unresolved and crashes;
- an integer constant beyond `i32` saturates (open question 1);
- `#include` followed by a newline before the name is "Include filename
  expected" (the original's scanner skips the newline without counting
  it, then reads the name).

**Tests.** `src/tests.rs` compiles a snippet per construct and compares
the listing (with jump targets as instruction indexes) with what this
document derives, including the complete and worked examples above, the
`JMP 0` cases, every table and the error positions; the lexer and parser
have their own tests for the token rules, the precedence levels and the
grammar's exclusions. `tests/white_chamber.rs` is the oracle: with the
White Chamber source pack unpacked under
`testdata/local/white-chamber/source/` (git-ignored) and the game's
packages under `testdata/local/white-chamber/unpacked/`, it compiles every
source script with `Natives::wme()`, the include root being the `data`
directory with `language` as a fallback, and compares it with the shipped
script; a difference is reported with the first differing offset and the
instructions around it in both. It skips when the data is absent.
