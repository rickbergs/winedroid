# Linkador recursivo de métodos DEX

O linkador começa nos quatro métodos de ciclo de vida do SukiSU e percorre as
referências `invoke-*` encontradas nos corpos DEX.

Um método entra no mesmo ELF quando:

1. possui corpo no APK;
2. está dentro da profundidade configurada;
3. não ultrapassa o limite total;
4. seus opcodes são aceitos pelo backend;
5. sua assinatura cabe na ABI atual do runtime.

O dispatch preserva o `method_id` original do DEX:

```text
method_id interno ligado → função nativa compilada
outro method_id          → stub externo
```

Todos os métodos ligados compartilham heap, campos estáticos, campos de
instância e `wd_last_result`.

## ABI inicial

Nesta etapa, o linkador internaliza com segurança:

- métodos estáticos sem argumentos;
- métodos de instância com `this` e no máximo um argumento adicional.

Assinaturas maiores permanecem em stub até a ABI do runtime ser generalizada.
