# Contribuindo

## Regras básicas

- não envie APKs proprietários ao repositório;
- todo parser precisa validar limites antes de indexar buffers;
- funcionalidades novas precisam de testes;
- stubs devem falhar explicitamente, nunca fingir sucesso;
- não copie código de projetos incompatíveis com Apache-2.0;
- preserve avisos de copyright de código Apache-2.0 incorporado.

## Antes do commit

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Commits

Use mensagens objetivas:

```text
apk: detect multidex entries
axml: parse UTF-8 string pools
dex: validate map offsets
runtime: implement const/4 opcode
```
