# Compilador nativo do WineDroid

Este componente não inicia Android, não virtualiza hardware e não interpreta
opcodes durante a execução do programa final.

## Pipeline atual

```text
APK ou DEX
  → leitura de class_data_item
  → extração de code_item
  → lowering do subconjunto Dalvik suportado
  → código C intermediário
  → Clang AOT
  → executável ELF x86_64
  → kernel Linux e CPU
```

O C é, neste estágio, uma representação intermediária prática e auditável.
Ele permite validar a semântica antes de introduzir um backend próprio ou
Cranelift/LLVM.

## Instruções iniciais

- `nop`;
- `move`, `move/from16`, `move/16`;
- `const/4`, `const/16`, `const`, `const/high16`;
- `return`, `return-void`;
- `goto`, `goto/16`, `goto/32`;
- comparações `if-*` e `if-*z`;
- operações inteiras;
- variantes `/2addr`, `/lit16` e `/lit8`.

## Limites atuais

O backend inicial aceita métodos sem parâmetros (`ins_size = 0`) e valores
inteiros. Objetos, arrays, strings, chamadas entre métodos, exceções, JNI e
APIs Android ainda precisam ser implementados.
