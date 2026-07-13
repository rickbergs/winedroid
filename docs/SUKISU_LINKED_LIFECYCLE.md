# Ciclo de vida ligado do SukiSU

Este marco transforma quatro métodos reais do SukiSU em um único executável
Linux:

```text
KernelSUApplication.<init>()
KernelSUApplication.onCreate()
MainActivity.<init>()
MainActivity.onCreate(Bundle)
```

## Estado compartilhado

O ELF possui uma única instância do runtime:

- contador de handles;
- heap de campos de instância;
- tabela de campos estáticos;
- `wd_last_result`;
- tabelas DEX de métodos, campos, strings e tipos.

O construtor e o `onCreate` recebem o mesmo handle de Application. O
construtor e o `onCreate` da Activity também recebem o mesmo handle.

## Limite deste marco

Os quatro corpos de métodos são código nativo no mesmo ELF. As chamadas para
métodos fora desse conjunto continuam usando `wd_invoke`, que registra a
chamada e retorna um valor compatível básico.

O próximo passo é seguir a árvore de chamadas e incluir métodos internos
adicionais no mesmo link.
