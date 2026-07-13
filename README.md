# WineDroid

**WineDroid** é uma camada experimental de compatibilidade para executar
aplicativos Android no Linux sem iniciar uma distribuição Android completa,
sem máquina virtual de sistema e sem contêiner Android.

O projeto lê APK e DEX, compila bytecode Dalvik antecipadamente e produz
executáveis Linux nativos. APIs Java e Android são substituídas
progressivamente por implementações sobre interfaces do Linux.

> **Estado atual:** compilador AOT, runtime nativo e linkador recursivo em
> desenvolvimento. Ainda não existe interface gráfica Android funcional.

## Estado comprovado

O WineDroid já consegue:

- analisar APK, Android Binary XML e multidex;
- resolver strings, tipos, campos, protótipos, métodos e classes;
- extrair `class_data_item` e `code_item`;
- compilar um subconjunto amplo de instruções Dalvik;
- produzir ELF PIE x86-64 com Clang;
- executar o resultado diretamente pelo kernel Linux;
- representar objetos por handles;
- compartilhar campos estáticos e de instância;
- executar `new-instance`, `iget/iput`, `sget/sput`, saltos, comparações,
  operações inteiras, arrays básicos, `invoke-*` e `move-result`;
- ligar `KernelSUApplication` e `MainActivity` em um único processo;
- seguir recursivamente chamadas internas do mesmo DEX quando a assinatura e
  os opcodes já são suportados pelo runtime atual;
- manter stubs apenas para chamadas externas ou ainda não implementadas.

## Integração SukiSU

O alvo atual é:

```text
SukiSU_v4.1.3_40796-release.apk
```

Os quatro métodos raiz são:

```text
KernelSUApplication.<init>()
KernelSUApplication.onCreate()
MainActivity.<init>()
MainActivity.onCreate(Bundle)
```

O linkador recursivo parte desses métodos, percorre as referências `invoke-*`
e internaliza métodos compatíveis no mesmo ELF.

```text
quatro métodos raiz
        ↓
árvore de chamadas DEX
        ↓
métodos internos suportados
        ↓
dispatch nativo compartilhado
        ↓
um único ELF x86-64
```

Chamadas para `android.*`, `java.*`, JNI ou métodos rejeitados continuam no
caminho de stub.

## Gerar o ELF recursivo do SukiSU

```bash
cargo run -p winedroid-compiler --bin winedroid-sukisu-recursive -- \
  ~/Downloads/SukiSU_v4.1.3_40796-release.apk \
  --output /tmp/winedroid-sukisu-recursive.elf \
  --emit-c /tmp/winedroid-sukisu-recursive.c \
  --max-depth 3 \
  --max-methods 32 \
  --run
```

O relatório mostra:

- métodos raiz;
- métodos internos realmente ligados;
- chamadas externas mantidas em stub;
- métodos rejeitados e o motivo;
- chamadas cortadas pelo limite de profundidade;
- profundidade alcançada.

## Pipeline

```text
APK
 ├─ AndroidManifest.xml ──→ parser AXML
 ├─ resources.arsc ───────→ resolvedor planejado
 ├─ classes*.dex
 │    ├─ índices e descritores
 │    ├─ corpos de métodos
 │    ├─ grafo de chamadas
 │    ├─ lowering Dalvik
 │    └─ C intermediário auditável
 └─ lib/<abi>/*.so ───────→ ponte Bionic/JNI planejada
                              ↓
                           Clang AOT
                              ↓
                         ELF x86-64
                              ↓
                     kernel Linux / CPU
```

O executável final não contém um loop interpretando opcodes. O C é um backend
intermediário temporário.

## Testar o workspace

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release --workspace
```

## Limitações atuais

- Activity ainda não abre uma janela;
- Jetpack Compose ainda não é renderizado;
- objetos e strings usam semântica simplificada;
- arrays e exceções são incompletos;
- não há coleta de lixo;
- `java.*` e `android.*` ainda não são reimplementados integralmente;
- JNI e Bionic ainda não funcionam;
- Binder, serviços Android e permissões ainda não funcionam;
- recursos, áudio, câmera, notificações e gráficos ainda não funcionam;
- não há tradução ARM/ARM64 para x86-64.

## Próximos marcos

1. Aumentar a cobertura de opcodes que bloqueiam a árvore do SukiSU.
2. Generalizar a ABI de chamadas para mais parâmetros e métodos estáticos.
3. Implementar strings, arrays, coleções e exceções reais.
4. Substituir stubs de `java.lang` e `java.util`.
5. Criar implementações Linux de `Context`, `Application` e `Activity`.
6. Abrir a primeira janela de Activity em Wayland.
7. Implementar JNI x86-64 e ponte Bionic.

## Segurança

APK é entrada não confiável. Não execute o WineDroid como root e não use APKs
não confiáveis em sistemas importantes.

## Filosofia

O objetivo não é esconder Android dentro de uma VM. O objetivo é oferecer ao
aplicativo as interfaces observáveis que ele espera, mapeando-as para o Linux
como uma camada de compatibilidade.

## Licença

Apache-2.0. Consulte [`LICENSE`](LICENSE).
