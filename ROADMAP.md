# Roadmap

## M0 — Fundação

- [x] workspace Rust;
- [x] CLI;
- [x] inspeção ZIP/APK;
- [x] reconhecimento inicial de AXML;
- [x] leitura do cabeçalho DEX;
- [x] inventário de bibliotecas nativas;
- [ ] corpus legal de APKs de teste;
- [ ] fuzzing inicial.

## M1 — Manifesto e recursos

- [ ] parser completo de Android Binary XML;
- [ ] string pool UTF-8/UTF-16;
- [ ] namespaces e atributos tipados;
- [ ] package name, SDK, permissões e componentes;
- [ ] parser mínimo de `resources.arsc`;
- [ ] comando `winedroid manifest`.

## M2 — Loader DEX

- [ ] map list;
- [ ] string/type/proto/field/method/class tables;
- [ ] encoded values;
- [ ] class data;
- [ ] code items;
- [ ] disassembler;
- [ ] comando `winedroid dex dump`.

## M3 — Execução DEX mínima

- [ ] frames e registradores;
- [ ] constantes e movimentos;
- [ ] aritmética;
- [ ] branches;
- [ ] invoke/return;
- [ ] objetos e arrays;
- [ ] exceções;
- [ ] executar um APK de teste sem `android.*`.

## M4 — Primeiro aplicativo visual

- [ ] `android.util.Log`;
- [ ] `Application`;
- [ ] `Activity`;
- [ ] `Intent`;
- [ ] `Looper` e fila de mensagens;
- [ ] janela Wayland;
- [ ] View/TextView/Button mínimos;
- [ ] entrada de teclado e ponteiro.

## M5 — JNI e Bionic

- [ ] carregador ELF Android x86_64;
- [ ] JNI Invocation/Native interfaces;
- [ ] wrappers libc/pthread/dl;
- [ ] OpenGL ES;
- [ ] primeiro APK com `.so`.

## M6 — Integração desktop

- [ ] instalação lógica;
- [ ] arquivos `.desktop`;
- [ ] ícones;
- [ ] notificações;
- [ ] clipboard;
- [ ] seletor de arquivos via portal;
- [ ] áudio PipeWire.

## M7 — Compatibilidade e desempenho

- [ ] suíte de regressão;
- [ ] cache de resolução;
- [ ] compilação de métodos quentes;
- [ ] multiprocessos;
- [ ] Binder compatível;
- [ ] sandbox por aplicativo.

## Compilação nativa AOT

- [x] Backend inicial Dalvik → C → ELF x86_64
- [x] Execução direta do ELF pelo kernel Linux
- [x] Extração de corpos de métodos de DEX e APK
- [ ] Objetos, chamadas entre métodos e exceções
- [ ] Reimplementação progressiva de `java.*` e `android.*`

## Ciclo de vida ligado

- [x] Quatro métodos reais do SukiSU compilados separadamente
- [x] Um único ELF para Application e MainActivity
- [x] Handles e campos compartilhados entre etapas do ciclo
- [ ] Compilar recursivamente a árvore de chamadas internas
- [ ] Primeira janela Linux para uma Activity

## Linkador recursivo seguro

- [x] Percorrer referências `invoke-*` a partir dos quatro métodos raiz
- [x] Internalizar métodos compatíveis no mesmo ELF
- [x] Dispatch por `method_id` original do DEX
- [x] Fallback para stubs externos
- [ ] ABI genérica para qualquer quantidade de argumentos
- [ ] Dispatch virtual baseado no tipo real do objeto

## ABI genérica

- [x] Encaminhar `argc + args[]`
- [x] Carregar todos os registradores de entrada Dalvik
- [x] Permitir métodos estáticos com argumentos
- [x] Permitir instâncias e construtores com múltiplos argumentos
- [x] Elevar limite prudencial para 512 code units
- [ ] Implementar exceções e aceitar métodos com `throw`
