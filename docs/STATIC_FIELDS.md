# Campos estáticos nativos

O WineDroid agora resolve índices da tabela `field_id` e compila os opcodes
primitivos de campos estáticos para armazenamento dentro do ELF Linux.

## Implementado

- `sget`, `sget-boolean`, `sget-byte`, `sget-char`, `sget-short`;
- `sput`, `sput-boolean`, `sput-byte`, `sput-char`, `sput-short`;
- resolução do índice para o descritor completo do campo;
- armazenamento `static int32_t` gerado no código nativo;
- valor padrão zero, como no carregamento inicial de classes Java;
- sobrescrita de valores pelo compilador para testes e futura inicialização;
- compilação de um getter real extraído do APK do Instagram.

`long`, `double`, referências de objetos e execução automática de `<clinit>`
ainda não fazem parte deste marco. O armazenamento criado aqui é a base para
a inicialização completa de classes nos próximos blocos.
