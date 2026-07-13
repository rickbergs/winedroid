# Bootstrap nativo do SukiSU

Este marco adiciona uma camada AOT para os elementos necessários no começo de
um aplicativo Android, sem iniciar Android e sem interpretar o DEX em tempo de
execução.

## Implementado

- handles de objetos;
- `new-instance` e `new-array`;
- campos de instância `iget` e `iput`;
- campos estáticos primitivos e de referência;
- strings e classes como handles;
- `invoke-direct`, `invoke-static`, `invoke-virtual`, `invoke-super` e
  `invoke-interface`, incluindo variantes `/range`;
- `move-result`, `move-result-object` e retornos de referência;
- stubs nativos auditáveis para APIs ainda não reimplementadas;
- análise da fronteira de compilação de `KernelSUApplication` e `MainActivity`.

Os stubs não significam que as APIs Android já funcionam. Eles permitem
executar o fluxo Dalvik compilado e registrar chamadas, enquanto cada API é
substituída progressivamente por uma implementação Linux real.

## Comando

```bash
winedroid-aot sukisu-frontier SukiSU.apk
```
