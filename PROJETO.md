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
