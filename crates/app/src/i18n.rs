//! Interface text in Portuguese (Brazil) and English.
//!
//! One method per string: the compiler flags a missing translation as a missing
//! method, never as a silent fallback.

use crate::config::{Lang, Quality};
use crate::netinfo::Kind;

#[derive(Clone, Copy)]
pub struct Tx(pub Lang);

impl Tx {
    fn p(self, pt: &'static str, en: &'static str) -> &'static str {
        match self.0 {
            Lang::Pt => pt,
            Lang::En => en,
        }
    }

    // --- shell ----------------------------------------------------------------
    pub fn nav_share(self) -> &'static str { self.p("Compartilhar este PC", "Share this PC") }
    pub fn nav_connect(self) -> &'static str { self.p("Conectar", "Connect") }
    pub fn nav_settings(self) -> &'static str { self.p("Ajustes", "Settings") }
    pub fn tagline(self) -> &'static str { self.p("Área de trabalho remota", "Remote desktop") }
    pub fn version(self, v: &str) -> String { format!("{} {v}", self.p("Versão", "Version")) }
    pub fn dismiss(self) -> &'static str { self.p("Fechar", "Dismiss") }

    // --- updates --------------------------------------------------------------
    pub fn upd_checking(self) -> &'static str { self.p("Procurando atualizações…", "Checking for updates…") }
    pub fn upd_latest(self) -> &'static str { self.p("Você está na versão mais recente", "You're up to date") }
    pub fn upd_disabled(self) -> &'static str { self.p("Atualizações desativadas", "Updates disabled") }
    pub fn upd_check_failed(self) -> &'static str { self.p("Não foi possível verificar atualizações", "Couldn't check for updates") }
    pub fn upd_updating(self) -> &'static str { self.p("Atualizando…", "Updating…") }
    pub fn upd_available(self, tag: &str) -> String { format!("{tag} {}", self.p("disponível", "is available")) }
    pub fn upd_now(self) -> &'static str { self.p("Atualizar agora", "Update now") }
    pub fn upd_overlay_title(self, tag: &str) -> String { format!("{} {tag}", self.p("Atualizando para", "Updating to")) }
    pub fn upd_overlay_hint(self) -> &'static str { self.p("O lindesk reinicia sozinho ao terminar.", "lindesk restarts by itself when this finishes.") }
    pub fn upd_busy(self) -> &'static str { self.p("Pare o compartilhamento e desconecte antes de atualizar.", "Stop sharing and disconnect before updating.") }
    pub fn upd_status_checking(self) -> &'static str { self.p("Verificando…", "Checking…") }
    pub fn upd_status_latest(self, v: &str) -> String { match self.0 { Lang::Pt => format!("A versão {v} é a mais recente."), Lang::En => format!("Version {v} is the latest.") } }
    pub fn upd_status_disabled(self) -> &'static str { self.p("Desativado por LINDESK_NO_UPDATE.", "Disabled by LINDESK_NO_UPDATE.") }
    pub fn upd_status_available(self, tag: &str) -> String { match self.0 { Lang::Pt => format!("{tag} disponível."), Lang::En => format!("{tag} is available.") } }
    pub fn upd_status_installing(self, tag: &str) -> String { match self.0 { Lang::Pt => format!("Instalando {tag}…"), Lang::En => format!("Installing {tag}…") } }
    pub fn upd_status_failed(self, e: &str) -> String { match self.0 { Lang::Pt => format!("Não foi possível atualizar: {e}"), Lang::En => format!("Could not update: {e}") } }

    // --- share page -----------------------------------------------------------
    pub fn share_title(self) -> &'static str { self.p("Compartilhar este computador", "Share this computer") }
    pub fn share_sub(self) -> &'static str { self.p("Deixe alguém ver e controlar esta tela.", "Let someone see and control this screen.") }
    pub fn st_off(self) -> (&'static str, &'static str) { (self.p("Compartilhamento desligado", "Not sharing"), self.p("Ligue para receber conexões.", "Start sharing to let someone connect.")) }
    pub fn st_starting(self) -> (&'static str, &'static str) { (self.p("Iniciando…", "Starting…"), self.p("Preparando a captura de tela.", "Preparing the screen capture.")) }
    pub fn st_waiting(self, enc: &str) -> (&'static str, String) { (self.p("Aguardando conexão", "Waiting for a connection"), match self.0 { Lang::Pt => format!("Pronto. Codificando com {enc}."), Lang::En => format!("Ready. Encoding with {enc}.") }) }
    pub fn st_connected(self, name: &str, fps: u32, mbps: f32) -> (String, String) {
        (match self.0 { Lang::Pt => format!("Conectado a {name}"), Lang::En => format!("Connected to {name}") }, format!("{fps} fps · {mbps:.1} Mbps"))
    }
    pub fn start_sharing(self) -> &'static str { self.p("Ligar compartilhamento", "Start sharing") }
    pub fn stop_sharing(self) -> &'static str { self.p("Parar compartilhamento", "Stop sharing") }
    pub fn first_time_hint(self) -> &'static str { self.p("Na primeira vez, o sistema pergunta qual tela ou janela compartilhar e lembra da escolha.", "The first time, your system asks which screen or window to share. It remembers your choice.") }
    pub fn id_title(self) -> &'static str { self.p("Seu ID", "Your ID") }
    pub fn id_hint(self) -> &'static str { self.p("Passe este ID para quem vai conectar. Você aprova cada pessoa antes de ela entrar.", "Give this ID to whoever will connect. You approve each person before they get in.") }
    pub fn copy_id(self) -> &'static str { self.p("Copiar ID", "Copy ID") }
    pub fn copied(self) -> &'static str { self.p("Copiado", "Copied") }
    pub fn id_changes(self) -> &'static str { self.p("O ID vem do seu endereço de rede e muda se você trocar de rede ou VPN.", "The ID comes from your network address and changes if you switch network or VPN.") }
    pub fn no_network_id(self) -> &'static str { self.p("Sem rede", "No network") }
    pub fn reach_title(self) -> &'static str { self.p("Quem consegue se conectar", "Who can connect") }
    pub fn reach(self, kind: Option<Kind>, iface: &str) -> String {
        match (kind, self.0) {
            (Some(Kind::Vpn), Lang::Pt) => format!("Pela VPN ({iface}): funciona para quem está na mesma VPN."),
            (Some(Kind::Vpn), Lang::En) => format!("Through the VPN ({iface}): works for anyone on the same VPN."),
            (Some(Kind::Lan), Lang::Pt) => format!("Pela rede local ({iface}): funciona para quem está na mesma rede (Wi‑Fi ou cabo)."),
            (Some(Kind::Lan), Lang::En) => format!("Through the local network ({iface}): works for anyone on the same network (Wi‑Fi or cable)."),
            (Some(Kind::Internet), Lang::Pt) => format!("Pela internet ({iface}): só funciona com a porta UDP liberada (redirecionada) no roteador."),
            (Some(Kind::Internet), Lang::En) => format!("Over the internet ({iface}): only works if the UDP port is forwarded on your router."),
            (Some(Kind::Virtual), Lang::Pt) => format!("Adaptador virtual ({iface}): provavelmente só serve para máquinas virtuais."),
            (Some(Kind::Virtual), Lang::En) => format!("Virtual adapter ({iface}): probably only useful for virtual machines."),
            (None, Lang::Pt) => "Nenhuma rede encontrada. Conecte-se à internet, a uma rede local ou a uma VPN.".to_string(),
            (None, Lang::En) => "No network found. Connect to the internet, a local network or a VPN.".to_string(),
        }
    }
    pub fn other_network(self) -> &'static str { self.p("Usar outra rede…", "Use another network…") }
    pub fn detect_public(self) -> &'static str { self.p("Detectar endereço público", "Detect public address") }
    pub fn detecting(self) -> &'static str { self.p("Detectando…", "Detecting…") }
    pub fn port_note(self, port: u16) -> String { match self.0 { Lang::Pt => format!("Porta UDP {port} · mude em Ajustes"), Lang::En => format!("UDP port {port} · change it in Settings") } }
    pub fn no_network(self) -> &'static str { self.p("Nenhuma rede encontrada. Conecte-se a uma rede primeiro.", "No network found. Connect to a network first.") }
    pub fn public_failed(self) -> &'static str { self.p("Não foi possível detectar o endereço público. Verifique sua internet.", "Could not detect the public address. Check your internet connection.") }
    pub fn start_failed(self, e: &str) -> String { match self.0 { Lang::Pt => format!("Não foi possível ligar o compartilhamento: {e}"), Lang::En => format!("Could not start sharing: {e}") } }
    pub fn host_crashed(self, how: &str) -> String { match self.0 { Lang::Pt => format!("O compartilhamento parou de repente ({how}). Detalhes: ~/.config/lindesk/logs/host.log"), Lang::En => format!("Sharing stopped unexpectedly ({how}). Details: ~/.config/lindesk/logs/host.log") } }

    // --- accept dialog --------------------------------------------------------
    pub fn req_title(self, name: &str) -> String { match self.0 { Lang::Pt => format!("{name} quer conectar"), Lang::En => format!("{name} wants to connect") } }
    pub fn req_body(self, control: bool) -> &'static str {
        if control {
            self.p("Esta pessoa poderá ver sua tela e controlar seu teclado e mouse.", "This person will be able to see your screen and control your keyboard and mouse.")
        } else {
            self.p("Esta pessoa poderá ver sua tela (sem controle).", "This person will be able to see your screen (view only).")
        }
    }
    pub fn req_code(self, code: &str) -> String { match self.0 { Lang::Pt => format!("Código de verificação: {code}"), Lang::En => format!("Verification code: {code}") } }
    pub fn req_code_hint(self) -> &'static str { self.p("Se a pessoa estiver na chamada com você, confira se o código é o mesmo nos dois computadores.", "If the person is on a call with you, check that both computers show the same code.") }
    pub fn accept(self) -> &'static str { self.p("Aceitar", "Accept") }
    pub fn always_allow(self) -> &'static str { self.p("Sempre permitir", "Always allow") }
    pub fn reject(self) -> &'static str { self.p("Recusar", "Reject") }

    // --- connect page ---------------------------------------------------------
    pub fn conn_title(self) -> &'static str { self.p("Conectar", "Connect") }
    pub fn conn_sub(self) -> &'static str { self.p("Controle outro computador que está rodando o lindesk.", "Control another computer running lindesk.") }
    pub fn conn_card(self) -> &'static str { self.p("Conectar a um computador", "Connect to a computer") }
    pub fn conn_hint(self) -> &'static str { self.p("Peça o ID de 10 dígitos para a outra pessoa. Ela vai precisar aceitar.", "Ask the other person for their 10-digit ID. They will need to accept.") }
    pub fn id_placeholder(self) -> &'static str { self.p("ID do outro computador (10 dígitos)", "ID of the other computer (10 digits)") }
    pub fn connect(self) -> &'static str { self.p("Conectar", "Connect") }
    pub fn disconnect(self) -> &'static str { self.p("Desconectar", "Disconnect") }
    pub fn sess_connecting(self) -> (&'static str, &'static str) { (self.p("Conectando…", "Connecting…"), self.p("Procurando o outro computador.", "Looking for the other computer.")) }
    pub fn sess_waiting(self, code: &str) -> (&'static str, String) { (self.p("Aguardando aprovação", "Waiting for approval"), match self.0 { Lang::Pt => format!("A outra pessoa precisa aceitar. Código de verificação: {code}"), Lang::En => format!("The other person must accept. Verification code: {code}") }) }
    pub fn sess_connected(self, fps: u32, ms: f32) -> (&'static str, String) { (self.p("Conectado", "Connected"), match self.0 { Lang::Pt => format!("{fps} fps · {ms:.0} ms de latência. F11 alterna tela cheia no visualizador."), Lang::En => format!("{fps} fps · {ms:.0} ms round trip. Press F11 in the viewer for full screen.") }) }
    pub fn recent_title(self) -> &'static str { self.p("Computadores recentes", "Recent computers") }
    pub fn recent_empty(self) -> &'static str { self.p("Os computadores aos quais você se conectar aparecem aqui.", "Computers you connect to show up here for one-click access.") }
    pub fn remove(self) -> &'static str { self.p("Remover", "Remove") }
    pub fn bad_id(self) -> &'static str { self.p("ID inválido: digite os 10 dígitos.", "Invalid ID: type the 10 digits.") }
    pub fn viewer_failed(self, how: &str) -> String { match self.0 { Lang::Pt => format!("O visualizador falhou ({how}). Detalhes: ~/.config/lindesk/logs/view.log"), Lang::En => format!("The viewer failed ({how}). Details: ~/.config/lindesk/logs/view.log") } }
    pub fn viewer_closed(self) -> &'static str { self.p("O visualizador fechou antes de conectar.", "The viewer closed before connecting.") }

    // --- settings -------------------------------------------------------------
    pub fn set_title(self) -> &'static str { self.p("Ajustes", "Settings") }
    pub fn set_sub(self) -> &'static str { self.p("O padrão funciona bem. Mude só se precisar.", "Defaults work well. Change them only if you need to.") }
    pub fn set_general(self) -> &'static str { self.p("Geral", "General") }
    pub fn language(self) -> (&'static str, &'static str) { (self.p("Idioma", "Language"), self.p("Muda na hora.", "Changes immediately.")) }
    pub fn set_sharing(self) -> &'static str { self.p("Compartilhamento", "Sharing") }
    pub fn quality(self) -> (&'static str, &'static str) { (self.p("Qualidade", "Quality"), self.p("Reduza se a conexão estiver lenta ou instável.", "Lower it if the connection is slow or unstable.")) }
    pub fn quality_name(self, q: Quality) -> &'static str {
        match q {
            Quality::Balanced => self.p("Equilibrada · 60 fps, até 20 Mbps", "Balanced · 60 fps, up to 20 Mbps"),
            Quality::HighQuality => self.p("Alta qualidade · 60 fps, até 40 Mbps", "High quality · 60 fps, up to 40 Mbps"),
            Quality::LowBandwidth => self.p("Pouca banda · 30 fps, até 6 Mbps", "Low bandwidth · 30 fps, up to 6 Mbps"),
        }
    }
    pub fn allow_control(self) -> (&'static str, &'static str) { (self.p("Permitir controle", "Allow control"), self.p("Deixa a outra pessoa usar seu teclado e mouse.", "Let the other person use your keyboard and mouse.")) }
    pub fn show_cursor(self) -> (&'static str, &'static str) { (self.p("Mostrar meu ponteiro", "Show my mouse pointer"), self.p("Desenha o ponteiro no vídeo compartilhado.", "Draw the pointer in the shared video.")) }
    pub fn port(self) -> (&'static str, &'static str) { (self.p("Porta de rede (UDP)", "Network port (UDP)"), self.p("Precisa estar liberada no firewall/roteador para uso pela internet. Reinicie o compartilhamento para aplicar.", "Must be open in your firewall/router for internet use. Restart sharing to apply.")) }
    pub fn remembered_screen(self) -> (&'static str, &'static str) { (self.p("Tela lembrada", "Remembered screen"), self.p("Esquece qual tela ou janela você compartilhou.", "Forget which screen or window you shared.")) }
    pub fn forget(self) -> &'static str { self.p("Esquecer", "Forget") }
    pub fn forgot_done(self) -> &'static str { self.p("Pronto. Da próxima vez o sistema pergunta qual tela usar.", "Done. Next time you share, the system asks which screen to use.") }
    pub fn encoder(self) -> (&'static str, &'static str) { (self.p("Codificador de vídeo", "Video encoder"), self.p("Automático escolhe o mais rápido do seu computador.", "Automatic picks the fastest one your computer has.")) }
    pub fn trusted_title(self) -> &'static str { self.p("Computadores confiáveis", "Trusted computers") }
    pub fn trusted_hint(self) -> &'static str { self.p("Entram sem pedir a sua aprovação.", "They get in without asking for your approval.") }
    pub fn trusted_empty(self) -> &'static str { self.p("Nenhum ainda. Use “Sempre permitir” ao aceitar uma conexão.", "None yet. Use “Always allow” when you accept a connection.") }
    pub fn set_viewing(self) -> &'static str { self.p("Visualização", "Viewing") }
    pub fn fullscreen(self) -> (&'static str, &'static str) { (self.p("Abrir em tela cheia", "Start in full screen"), self.p("Abre telas remotas ocupando todo o monitor.", "Open remote screens covering the whole display.")) }
    pub fn decoder(self) -> (&'static str, &'static str) { (self.p("Decodificador de vídeo", "Video decoder"), self.p("Automático escolhe o mais rápido do seu computador.", "Automatic picks the fastest one your computer has.")) }
    pub fn set_updates(self) -> &'static str { self.p("Atualizações", "Updates") }
    pub fn auto_update(self) -> (&'static str, &'static str) { (self.p("Atualizar automaticamente", "Update automatically"), self.p("Procura ao abrir o lindesk e instala versões novas.", "Check when lindesk opens and install new versions.")) }
    pub fn status(self) -> &'static str { self.p("Situação", "Status") }
    pub fn check_now(self) -> &'static str { self.p("Verificar agora", "Check now") }
    pub fn auto_label(self) -> &'static str { self.p("Automático (recomendado)", "Automatic (recommended)") }
    pub fn va_label(self) -> &'static str { self.p("Placa Intel / AMD (VA-API)", "Intel / AMD graphics (VA-API)") }
    pub fn nv_label(self, dec: bool) -> &'static str { if dec { self.p("Placa NVIDIA (NVDEC)", "NVIDIA graphics (NVDEC)") } else { self.p("Placa NVIDIA (NVENC)", "NVIDIA graphics (NVENC)") } }
    pub fn cpu_label(self) -> &'static str { self.p("Processador (mais lento, funciona em tudo)", "Processor (slower, works everywhere)") }

    // --- components -----------------------------------------------------------
    pub fn missing_title(self) -> &'static str { self.p("Faltam componentes necessários", "Some required components are missing") }
    pub fn missing_footer(self) -> &'static str { self.p("Instale com o gerenciador de pacotes e reabra o lindesk.", "Install them with your package manager, then reopen lindesk.") }

    /// Messages from the host/viewer libraries are English; translate the ones people actually meet.
    pub fn localize(self, msg: &str) -> String {
        if self.0 == Lang::En {
            return msg.to_string();
        }
        const MAP: [(&str, &str); 7] = [
            ("No answer from", "Sem resposta do outro computador. Ele está compartilhando? A rede ou VPN está conectada?"),
            ("did not accept the connection", "A outra pessoa recusou a conexão."),
            ("did not answer the request in time", "A outra pessoa não respondeu ao pedido a tempo."),
            ("identity of this computer changed", "A identidade deste computador mudou desde a última conexão. Se ele foi reinstalado, remova-o dos recentes e conecte de novo; senão, alguém pode estar se passando por ele."),
            ("Connection lost", "Conexão perdida: o outro computador parou de enviar vídeo."),
            ("incompatible version", "O outro computador usa uma versão incompatível. Atualize os dois."),
            ("Input control unavailable", "Controle de teclado e mouse indisponível: sem acesso a /dev/uinput (adicione-se ao grupo input e entre de novo)."),
        ];
        MAP.iter().find(|(k, _)| msg.contains(k)).map_or_else(|| msg.to_string(), |(_, v)| v.to_string())
    }
}
