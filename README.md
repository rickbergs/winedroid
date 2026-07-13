# WineDroid

**WineDroid** é uma camada experimental de compatibilidade para executar
aplicativos Android no Linux sem iniciar uma distribuição Android completa,
sem máquina virtual e sem contêiner de sistema Android.

> Estado atual: fundação do projeto. O código ainda não executa aplicativos.

## Primeiro marco

A versão `0.1.0` começa pelo carregador de pacotes:

- abre APKs como arquivos ZIP;
- reconhece `AndroidManifest.xml`;
- identifica Android Binary XML;
- valida o cabeçalho de arquivos `classes*.dex`;
- lista bibliotecas nativas por ABI;
- reconhece `resources.arsc`, recursos, assets e assinaturas APK v1;
- produz relatório humano ou JSON;
- nunca extrai nem executa código do APK.

## Compilar

```bash
cargo build --workspace
cargo test --workspace
```

## Usar

```bash
cargo run -p winedroid-cli -- doctor
cargo run -p winedroid-cli -- inspect caminho/app.apk
cargo run -p winedroid-cli -- inspect caminho/app.apk --json
```

## Objetivo técnico

O fluxo pretendido é:

```text
APK
 ├─ AndroidManifest.xml → parser AXML e modelo de componentes
 ├─ resources.arsc      → resolvedor de recursos
 ├─ classes*.dex        → loader e runtime DEX
 └─ lib/<abi>/*.so      → ponte Bionic/JNI para o host Linux
                              ↓
                  APIs Android reimplementadas
                              ↓
          Wayland + PipeWire + D-Bus + Linux
```

O WineDroid não pretende fingir que um APK é ELF. Ele fornecerá ao aplicativo
as interfaces observáveis que ele esperaria do Android, da mesma forma
conceitual que uma camada de compatibilidade fornece APIs de outro sistema.

## Estratégia

1. Parser seguro de APK, AXML, `resources.arsc` e DEX.
2. Interpretador DEX mínimo capaz de executar código sem APIs Android.
3. Runtime de objetos, classes, exceções e threads.
4. Primeiras APIs: `android.util.Log`, `Context`, `Intent` e ciclo de Activity.
5. Views básicas convertidas para janelas Wayland.
6. JNI e bibliotecas nativas x86_64.
7. Ponte Bionic → glibc.
8. Áudio, entrada, rede, notificações e aceleração gráfica.
9. Compatibilidade progressiva por testes reais e regressão.

## Não objetivos iniciais

- Google Play Services;
- Play Integrity;
- DRM e Widevine;
- aplicativos bancários;
- tradução ARM → x86_64;
- compatibilidade universal imediata.

## Segurança

APK é entrada não confiável. O WineDroid deve tratar todos os tamanhos,
offsets, tabelas e nomes como potencialmente maliciosos. Não execute o
WineDroid como root.

## Trabalho anterior

O projeto `apkenv` demonstrou que partes de APKs, especialmente jogos nativos,
podem ser adaptadas para hosts Linux usando linker Android, wrappers JNI,
hooks de Bionic e módulos específicos por aplicativo. O WineDroid pretende
partir para uma arquitetura geral baseada em DEX e APIs Android, sem depender
de módulos manuais por aplicativo.

## Licença

Apache-2.0. Consulte `LICENSE`.
