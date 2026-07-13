# WineDroid

**WineDroid** é uma camada experimental de compatibilidade para executar
aplicativos Android no Linux **sem iniciar uma distribuição Android completa,
sem máquina virtual de sistema e sem contêiner Android**.

O projeto analisa APK e DEX, compila bytecode Dalvik antecipadamente e produz
executáveis Linux nativos. As APIs Java e Android são reimplementadas
progressivamente sobre interfaces do Linux.

> **Estado atual:** protótipo de compilador AOT e runtime nativo. Ainda não
> executa APKs completos nem desenha a interface Android.

## Marco atual

O WineDroid já consegue:

- abrir APKs e validar sua estrutura ZIP;
- analisar `AndroidManifest.xml` em Android Binary XML;
- identificar pacote, versão, SDK, Application, launcher, Activities e
  permissões;
- indexar strings, tipos, protótipos, campos, métodos e classes DEX;
- localizar corpos reais de métodos em `class_data_item` e `code_item`;
- compilar um subconjunto de instruções Dalvik para C intermediário;
- usar Clang AOT para produzir ELF PIE x86-64;
- executar o ELF diretamente pelo kernel Linux;
- representar objetos por handles;
- manter campos estáticos e campos de instância;
- resolver strings, tipos, campos e referências de métodos;
- tratar operações inteiras, saltos, comparações, arrays básicos,
  `new-instance`, `iget/iput`, `sget/sput`, `invoke-*`, `move-result` e
  retornos;
- registrar chamadas ainda não implementadas por stubs nativos auditáveis.

### SukiSU Manager

O alvo de integração atual é o
`SukiSU_v4.1.3_40796-release.apk`.

Foram validados os métodos reais:

```text
KernelSUApplication.<init>()
KernelSUApplication.onCreate()
MainActivity.<init>()
MainActivity.onCreate(Bundle)
```

Cada método já foi compilado e executado separadamente como ELF x86-64. O
marco seguinte liga os quatro em **um único processo nativo**, com:

- uma instância compartilhada de `KernelSUApplication`;
- uma instância compartilhada de `MainActivity`;
- heap compartilhado;
- campos estáticos compartilhados;
- campos de instância compartilhados;
- execução sequencial do ciclo de vida inicial.

As chamadas externas ao conjunto ligado ainda passam pelo runtime de stubs.
Isso significa que o fluxo Dalvik é real, mas `Activity`, Compose, JNI e APIs
do Android ainda não têm implementação completa.

## Pipeline

```text
APK
 ├─ AndroidManifest.xml ──→ parser AXML
 ├─ resources.arsc ───────→ resolvedor de recursos em desenvolvimento
 ├─ classes*.dex
 │    ├─ índices e descritores
 │    ├─ class_data_item / code_item
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

Não existe um loop interpretando opcodes no executável final. O C é uma
representação intermediária temporária; backends diretos como LLVM,
Cranelift ou geração própria poderão substituí-lo.

## Compilar e testar

Requisitos atuais:

- Rust;
- Cargo;
- Clang;
- `file`;
- `readelf`.

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release --workspace
```

## Inspecionar um APK

```bash
cargo run -p winedroid-cli -- doctor
cargo run -p winedroid-cli -- inspect caminho/app.apk
cargo run -p winedroid-cli -- inspect caminho/app.apk --json
```

## Compilar um método para ELF

```bash
cargo run -p winedroid-compiler --bin winedroid-aot -- \
  compile-apk caminho/app.apk \
  --method 'Lpacote/Classe;->metodo()I' \
  --output /tmp/metodo-nativo \
  --run
```

## Ligar o ciclo inicial do SukiSU

```bash
cargo run -p winedroid-compiler --bin winedroid-sukisu-link -- \
  ~/Downloads/SukiSU_v4.1.3_40796-release.apk \
  --output /tmp/winedroid-sukisu-linked \
  --emit-c /tmp/winedroid-sukisu-linked.c \
  --run
```

O resultado esperado é um único ELF x86-64 que executa os quatro métodos do
ciclo de vida no mesmo processo e termina com:

```text
WineDroid: SukiSU linked lifecycle completed
```

## O que ainda não funciona

- janela ou interface gráfica Android;
- Jetpack Compose;
- dispatch real de todos os métodos DEX chamados;
- coleta de lixo e semântica completa de referências;
- exceções Android completas;
- threads e sincronização compatíveis;
- `resources.arsc` completo e layouts;
- JNI;
- carregamento de bibliotecas Android/Bionic;
- ponte Bionic → glibc;
- Binder e serviços Android;
- áudio, câmera, notificações e aceleração gráfica;
- Google Play Services, Play Integrity, DRM e Widevine;
- tradução ARM/ARM64 → x86-64.

## Próximos marcos

1. Expandir a árvore de chamadas internas e substituir stubs por corpos DEX
   compilados.
2. Implementar `java.lang`, strings, coleções e exceções com semântica real.
3. Implementar `Application`, `Context`, `Activity` e ciclo de vida nativos.
4. Criar a primeira janela Linux para uma Activity.
5. Mapear Views/Compose para Wayland e entrada Linux.
6. Implementar JNI x86-64 e compatibilidade Bionic.
7. Adicionar PipeWire, D-Bus, rede, arquivos e notificações.

## Segurança

APK é entrada não confiável. Todos os tamanhos, offsets, tabelas, strings,
nomes e índices devem ser validados.

**Não execute o WineDroid como root.**

O projeto ainda é experimental e não deve ser usado para executar APKs não
confiáveis em sistemas importantes.

## Filosofia

O objetivo não é iniciar Android escondido. O objetivo é oferecer ao
aplicativo as interfaces observáveis que ele espera, mapeando-as para o Linux,
de forma semelhante ao conceito de uma camada de compatibilidade.

## Licença

Apache-2.0. Consulte [`LICENSE`](LICENSE).
