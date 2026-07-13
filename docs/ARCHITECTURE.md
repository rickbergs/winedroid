# Arquitetura do WineDroid

## Princípio

O WineDroid não executará um sistema Android escondido. Ele implementará uma
fronteira compatível com o aplicativo e usará serviços nativos do Linux por
baixo.

## Componentes planejados

### 1. Package loader

Responsável por:

- ZIP/APK;
- assinaturas;
- manifesto AXML;
- recursos;
- seleção de ABI;
- instalação lógica e diretórios privados.

### 2. DEX runtime

Primeira implementação: interpretador por registradores.

Responsabilidades:

- carregamento de strings, tipos, métodos e classes;
- verificação de offsets e índices;
- frames de métodos e registradores;
- dispatch de opcodes;
- objetos, arrays e exceções;
- resolução de métodos virtuais e interfaces.

A otimização JIT/AOT só deve começar depois de existir correção funcional e
uma suíte forte de testes.

### 3. Java/Android class libraries

Camadas separadas:

```text
java.* / javax.*          biblioteca de linguagem
dalvik.*                  compatibilidade do runtime
android.*                 framework observável pelos apps
com.android.internal.*    somente quando inevitável
```

Cada método deve ser classificado como:

- implementado nativamente;
- adaptado para uma API Linux;
- stub explícito;
- não suportado.

### 4. WineDroid server

Processo opcional para estado compartilhado:

- package manager;
- permissões;
- intents entre processos;
- serviços;
- notificações;
- clipboard;
- objetos Binder futuros.

Aplicativos simples devem poder começar em processo único.

### 5. Host bridges

```text
Android                 Linux
Activity/View           Wayland
AudioTrack              PipeWire
NotificationManager     D-Bus / portal
ClipboardManager        Wayland / portal
Socket                  sockets POSIX
SharedPreferences       arquivos transacionais
SQLite                  SQLite do host
OpenGL ES               EGL/OpenGL/Vulkan
```

### 6. Native bridge

Ordem de suporte:

1. APK puramente DEX;
2. bibliotecas x86_64;
3. bibliotecas x86;
4. ARM somente por tradutor externo e opcional.

A ponte deve separar:

- linker ELF Android;
- ABI Bionic;
- JNI;
- símbolos gráficos;
- pthread e TLS;
- syscalls incompatíveis.

## Modelo de instalação

Diretório sugerido:

```text
~/.local/share/winedroid/
├── packages/
│   └── com.example.app/
│       ├── base.apk
│       ├── code-cache/
│       ├── data/
│       └── metadata.json
├── runtime/
└── state/
```

## Fronteiras de segurança

- nunca confiar em tamanho ou offset vindo do APK;
- limites de memória e profundidade;
- sem extração com path traversal;
- processo sandbox por aplicativo;
- nenhuma permissão de host implícita;
- fuzzing dos parsers antes do runtime executar APKs públicos.
