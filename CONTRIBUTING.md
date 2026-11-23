# Contribuindo

Obrigado por ajudar! Contribuições em português ou inglês são bem-vindas.

## O jeito mais fácil: um perfil de controle novo

1. Instale o GameSir Bridge e conecte o controle.
2. Rode `gamesir-bridge calibrate` e siga os 21 passos.
3. Copie o arquivo de `/etc/gamesir-bridge/profiles/` para `profiles/` no repositório, dê um
   `id` e um `name` bons e, se fizer sentido, `notes` com as limitações.
4. Abra um PR usando o modelo "Controle novo" e cole a saída de `gamesir-bridge list`.

O formato está em [docs/profiles.md](docs/profiles.md).

## Ambiente

- Rust 1.89 ou mais novo (`rustup` recomendado).
- `dbus-daemon` para o teste de ponta a ponta do D-Bus (pacote `dbus`; sem ele o teste é pulado).
- Para testar no hardware: `/dev/uinput` e acesso aos `/dev/input/event*` (root).

```bash
cargo build
cargo test --workspace
cargo fmt --all && cargo clippy --workspace --all-targets
```

Rodando o serviço sem instalar, no barramento de sessão:

```bash
sudo systemctl stop gamesir-bridge            # se estiver instalado
sudo -E target/debug/gamesir-bridged --session-bus --profiles profiles --config /tmp/gsb -v
target/debug/gamesir-bridge --session-bus status
```

## Organização

| Caminho | O quê |
| --- | --- |
| `crates/core` | Mapeamento, perfis, calibração e descoberta (sysfs). Sem I/O de dispositivo: tudo testável. |
| `crates/daemon` | `gamesir-bridged` (workers evdev/uinput, hotplug, D-Bus) e a CLI `gamesir-bridge`. |
| `profiles/` | Perfis oficiais. |
| `data/` | udev, systemd, D-Bus, polkit, ícone, AppImage. |
| `tools/` | Scripts de build do AppImage, release e geração dos valores de referência. |

## O comportamento do Mapper não muda sem querer

O `Mapper` é um porte do script Python que foi testado no hardware. O teste
`crates/core/tests/python_golden.rs` compara 600 estados com valores gerados pelo Mapper em
Python (`tools/gen_python_golden.py`), e a CI regenera esses valores para conferir. Se uma
mudança altera a saída de propósito, explique no PR.

## Commits e PRs

- Mensagens no estilo [Conventional Commits](https://www.conventionalcommits.org/pt-br/)
  (`feat:`, `fix:`, `docs:`, `chore:`...), que facilitam o changelog.
- Toda mudança visível para o usuário entra em `CHANGELOG.md`, na seção `[Unreleased]`.
- A CI precisa passar: formatação, clippy sem avisos, testes, shellcheck e o AppImage.
  Cada PR gera um AppImage para teste nos artefatos da execução.

## Lançando uma versão

```bash
tools/release.sh 0.3.0      # atualiza versões e CHANGELOG, faz o commit e a tag
git push && git push origin v0.3.0
```

A tag dispara `.github/workflows/release.yml`, que testa, compila os binários estáticos e
publica a release com o AppImage (e o `.zsync` para atualização), um tarball e os checksums.
As notas vêm da seção da versão no `CHANGELOG.md`.
