# WineDroid

**WineDroid** é uma camada experimental de compatibilidade para executar
aplicativos Android no Linux sem iniciar Android completo, sem máquina virtual
de sistema e sem contêiner Android.

O projeto analisa APK e DEX, compila bytecode Dalvik antecipadamente e produz
executáveis Linux nativos.

> **Estado atual:** compilador AOT, runtime nativo, ciclo de vida ligado,
> linkador recursivo e ABI genérica. Ainda não existe interface gráfica Android
> funcional.

## Estado comprovado

O WineDroid já consegue:

- analisar APK, Android Binary XML e DEX;
- resolver strings, tipos, campos, métodos e classes;
- extrair corpos reais de métodos;
- compilar Dalvik para C e ELF PIE x86-64;
- executar o resultado diretamente pelo kernel Linux;
- compartilhar objetos, campos e estado;
- ligar `KernelSUApplication` e `MainActivity` no mesmo processo;
- seguir chamadas internas recursivamente;
- encaminhar qualquer quantidade de registradores por `argc + args[]`;
- executar métodos estáticos com argumentos e instâncias com múltiplos
  parâmetros.

## Marco SukiSU

O marco anterior atingiu:

```text
4 métodos raiz
23 métodos internos ligados
54 chamadas externas
30 métodos rejeitados
profundidade 3
```

A ABI genérica remove a limitação anterior de apenas dois argumentos. Cada
função ligada agora recebe:

```c
uint32_t argc, const wd_value *args
```

Os registradores de entrada Dalvik são carregados no final do frame. O limite
prudencial subiu de 96 para 512 code units. Métodos com `throw` explícito
continuam fora do grafo até a implementação correta de exceções.

## Gerar o ELF recursivo

```bash
cargo run -p winedroid-compiler --bin winedroid-sukisu-recursive --   ~/Downloads/SukiSU_v4.1.3_40796-release.apk   --output /tmp/winedroid-sukisu-generic-abi.elf   --emit-c /tmp/winedroid-sukisu-generic-abi.c   --max-depth 3   --max-methods 96   --run
```

## Testar

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release --workspace
```

## Limitações

- nenhuma janela de Activity;
- Jetpack Compose ainda não renderiza;
- dispatch virtual ainda usa o `method_id` estático;
- strings, arrays, coleções e exceções são incompletos;
- JNI, Bionic, Binder e serviços Android ainda não funcionam;
- não há tradução ARM/ARM64 para x86-64.

## Próximos marcos

1. Implementar exceções e aceitar métodos com `throw`.
2. Cobrir os opcodes restantes.
3. Implementar dispatch virtual por tipo real.
4. Substituir stubs de `java.lang` e `java.util`.
5. Abrir a primeira janela Wayland.
6. Implementar JNI x86-64 e ponte Bionic.

## Segurança

APK é entrada não confiável. Não execute o WineDroid como root.

## Licença

Apache-2.0. Consulte [`LICENSE`](LICENSE).

### Labels Dalvik sem predecessores

O C intermediário preserva os labels dos blocos Dalvik. Depois do lowering,
alguns blocos podem ficar sem predecessores e o Clang reporta
`-Wunused-label`. O backend mantém `-Werror`, desativando exclusivamente essa
categoria, que não altera a semântica do ELF gerado.

### `packed-switch` Dalvik

O backend reconhece o opcode `0x2b`, interpreta o
`packed-switch-payload` e gera um `switch` C com destinos validados.
Payloads DEX são tratados como dados e não recebem labels executáveis.

### Limite de métodos internos

O linker recursivo aceita métodos internos com até 1024 code units. A ampliação permite traduzir métodos reais maiores, como o dispatcher com `packed-switch`, mas continua rejeitando métodos que contenham `throw` enquanto o tratamento de exceções Dalvik não estiver pronto.

### Caminhos com `throw`

Métodos que contêm o opcode Dalvik `throw` (`0x27`) podem entrar no grafo recursivo. O backend já traduz esse opcode para `wd_throw`; portanto, o processo só encerra com status 103 quando o caminho de exceção é realmente executado.

### Teto do grafo recursivo

O linker recursivo admite até 192 métodos internos por padrão. O limite anterior de 96 passou a truncar o grafo antes de métodos maiores alcançáveis, mesmo sem atingir o limite de profundidade.
