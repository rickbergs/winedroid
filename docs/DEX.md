# DEX indexing in WineDroid

M2 adds an owned, bounds-checked structural index for classic Dalvik Executable files.

Parsed structures:

- `string_id_item` and MUTF-8 `string_data_item`;
- `type_id_item`;
- `proto_id_item` and parameter `type_list`;
- `field_id_item`;
- `method_id_item`;
- `class_def_item`, superclass, interfaces and source file.

This milestone does not execute bytecode yet. The next runtime-facing layers are:

1. `class_data_item` and encoded methods;
2. `code_item` and instruction decoding;
3. register frames and a minimal interpreter;
4. native WineDroid implementations of selected `java.*` and `android.*` APIs.

The parser rejects out-of-bounds offsets, invalid table indices, malformed ULEB128 values and invalid MUTF-8 sequences before they can reach the future runtime.
