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

## Controle de fluxo com `packed-switch`

O opcode Dalvik `0x2b` usa o formato `31t`: o offset aponta para um
`packed-switch-payload`. O backend lê `size`, `first_key` e os offsets
relativos à instrução de switch, valida cada destino e emite um
`switch ((int32_t)valor)` em C.

## Limite de tamanho dos métodos

Métodos internos com até 1024 code units podem entrar no grafo recursivo. A verificação de segurança para o opcode `throw` (`0x27`) permanece ativa.

## Métodos com caminhos de exceção

A mera presença de `throw` (`0x27`) não impede mais a ligação do método. O lowering existente chama `wd_throw` apenas quando o bloco correspondente é alcançado. Tratamento Dalvik de `try/catch` ainda não foi implementado.

## Tamanho do grafo

O limite padrão do linker foi ampliado de 96 para 192 métodos. Métodos incompatíveis continuam sendo rejeitados individualmente, sem impedir a expansão dos demais ramos do grafo.
