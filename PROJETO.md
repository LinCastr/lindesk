# PROJETO.md — lindesk

Remote desktop (Parsec/AnyDesk style) in Rust with a **Wayland host**, built for low latency.
Workspace: `crates/proto` (wire format, AEAD, FEC, reassembly), `crates/host`, `crates/client`.

## Como verificar
| O quê | Comando | Esperado |
|---|---|---|
| Protocolo (cripto, FEC, remontagem) | `cargo test -p lindesk-proto` | 9 testes ok |
| Build | `cargo build --release` | sem erros/avisos |
| Ponta a ponta sem tocar na tela | host: `lindesk-host --source test --no-input --listen 127.0.0.1:17000 --key-file K` e cliente: `lindesk-client 127.0.0.1:17000 --key-file K --headless --seconds 8` | `decoded N frames` com N > 0 |
| Fonte PipeWire sem portal | `gst-launch-1.0 videotestsrc is-live=true ! pipewiresink mode=provide stream-properties="props,media.class=Video/Source,node.name=x"` e `--source node:<id>` (id via `pw-dump`) | idem |

## Regras e armadilhas
- **Nunca teste com `--source portal` ou sem `--no-input` na máquina do dono enquanto ele usa o PC**: o portal abre um seletor na tela dele e o uinput move o mouse de verdade.
- Teste de desempenho em máquina carregada (jogo rodando) é inútil: o fps medido é da CPU disputada, não do pipeline. Meça em máquina ociosa.
- Hospedeiro usa GStreamer (`pipewiresrc`, `vapostproc`, `vah264enc`/`nvh264enc`/`x264enc`). Cliente decodifica com `vah264dec`/`nvh264dec`/`avdec_h264`.
- `pipewiresrc` toma posse do fd: o host entrega um `dup` a cada início de pipeline.
- O pipeline do host só roda com cliente conectado (não gasta GPU em idle); timeout de 5 s sem pacote autenticado.
- Entrada: uinput (precisa de escrita em `/dev/uinput`, grupo `input`). Mouse é absoluto 0..65535 sobre a tela capturada: só correto se a tela capturada for o desktop inteiro/monitor na origem (multi-monitor: pendente).
- Chave pré-compartilhada de 32 bytes (hex) em `~/.config/lindesk/key` (0600). Sem chave válida o host ignora o pacote; replay é rejeitado por janela deslizante.
- Não use `pkill -f`: casa com o próprio shell. Mate por PID.

## App, atualizador e releases (v0.1+)
- Binário único `lindesk` (crates/app): sem argumentos abre a janela iced; `host`/`view` são os processos filhos que a janela lança (eventos JSON no stdout, o host para quando o stdin fecha). Libs: `crates/host` (`lindesk_host::run`), `crates/client` (`lindesk_viewer::run`), `crates/proto`.
- Código de conexão `LD-…` (proto/code.rs) = endereço + chave; é segredo. Config em `~/.config/lindesk/config.json` (0600).
- Atualizador (`crates/app/src/update.rs`): GitHub API `releases/latest` → baixa `lindesk-linux-x86_64` + `.minisig` → verifica com a chave pública embutida (`PUBLIC_KEY`) → roda `--version` no novo → `rename` por cima do exe → `exec`. Só roda na abertura da janela, nunca durante sessão. `LINDESK_NO_UPDATE=1` desliga.
- Release: tag `vX.Y.Z` igual à versão do `Cargo.toml` → `.github/workflows/release.yml` compila (ubuntu-22.04, libs do sistema, sem empacotar nada), assina com `tools/sign` e publica binário, `.minisig`, `SHA256SUMS`, `install.sh`, `lindesk.png`.
- Segredos do repo: `MINISIGN_KEY` e `MINISIGN_PASSWORD`. Cópia local da chave privada e senha em `~/.config/lindesk-release/` (fora do repo, nunca comitar). Trocar a chave exige trocar `PUBLIC_KEY` no app e no `install.sh` e lançar uma versão que os usuários consigam instalar à mão (o atualizador antigo rejeita assinatura da chave nova).
- Instalador: `install.sh` (curl | bash) instala em `~/.local/bin`, sem root, sem `ld.so`/`LD_LIBRARY_PATH`; confere SHA256 e, se houver `minisign`, a assinatura.
- Nunca teste a janela com `--source portal` + input ligado em loopback: o cliente controla a própria tela.
