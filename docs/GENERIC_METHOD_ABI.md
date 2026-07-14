# ABI genérica de métodos ligados

A ABI antiga expunha cada função como:

```c
wd_value method(wd_value this_value, wd_value arg0);
```

A ABI atual usa:

```c
wd_value method(uint32_t argc, const wd_value *args);
```

## Mapeamento

```text
incoming_start = registers_size - ins_size
v[incoming_start + i] = args[i]
```

Isso cobre `this`, argumentos primitivos, referências e palavras de valores
wide.

## Proteções

- `ins_size` não pode exceder `registers_size`;
- argumentos ausentes recebem zero;
- métodos acima de 512 code units são adiados;
- métodos com `throw` explícito continuam adiados;
- profundidade recursiva em runtime permanece limitada.

## Labels sem predecessores

Todos os labels Dalvik são preservados no C intermediário. Blocos que ficam
inalcançáveis após o lowering podem não possuir saltos de entrada. Por isso,
somente `-Wunused-label` é dispensado; os demais avisos do Clang continuam
tratados como erro por `-Werror`.
