# -*- coding: utf-8 -*-
"""
Racha de Cadeiras - servidor da rede local.

Serve o jogo em http://<seu-ip>:8080 e sincroniza, por WebSocket,
a posicao de todo mundo na pista e o placar de recordes.

So usa a biblioteca padrao do Python: nao precisa instalar nada.
"""

import base64
import hashlib
import json
import os
import socket
import struct
import sys
import threading
import time
import webbrowser

try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass

AQUI = os.path.dirname(os.path.abspath(__file__))
HTML = os.path.join(AQUI, "jogo.html")
DADOS = os.path.join(AQUI, "jogadores.json")

PORTA_INICIAL = 8080
GUID = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
TICK = 1.0 / 15.0          # quantas vezes por segundo o servidor manda as posicoes
MAX_QUADRO = 1 << 20       # 1 MiB por mensagem, de sobra

trava = threading.Lock()
clientes = {}              # conn -> estado do cliente
perfis = {}                # id do jogador -> perfil salvo


# --------------------------------------------------------------------------
# persistencia
# --------------------------------------------------------------------------
def carrega():
    global perfis
    try:
        with open(DADOS, "r", encoding="utf-8") as f:
            d = json.load(f)
        if isinstance(d, dict):
            perfis = d
    except FileNotFoundError:
        perfis = {}
    except Exception as e:
        print("  aviso: nao consegui ler jogadores.json (%s); comecando do zero" % e)
        perfis = {}


def grava():
    """Chame sempre com a trava tomada."""
    tmp = DADOS + ".tmp"
    try:
        with open(tmp, "w", encoding="utf-8") as f:
            json.dump(perfis, f, ensure_ascii=False, indent=1)
        os.replace(tmp, DADOS)
    except Exception as e:
        print("  aviso: nao consegui salvar jogadores.json (%s)" % e)


def perfil_novo(cor):
    return {
        "nick": "",
        "coins": 120,
        "up": {"motor": 0, "pneu": 0, "susp": 0, "quadro": 0},
        "best": {},
        "corridas": 0,
        "cor": cor or "#ff6a1f",
    }


def monta_board():
    """Chame sempre com a trava tomada."""
    linhas = []
    for pid, p in perfis.items():
        linhas.append({
            "id": pid,
            "nick": (p.get("nick") or "piloto")[:14],
            "best": p.get("best") or {},
        })
    return linhas[:200]


# --------------------------------------------------------------------------
# WebSocket (RFC 6455, o minimo necessario)
# --------------------------------------------------------------------------
def le_exato(conn, n):
    buf = b""
    while len(buf) < n:
        pedaco = conn.recv(n - len(buf))
        if not pedaco:
            raise ConnectionError("fechou")
        buf += pedaco
    return buf


def le_quadro(conn):
    b1, b2 = le_exato(conn, 2)
    opcode = b1 & 0x0F
    mascarado = b2 & 0x80
    n = b2 & 0x7F
    if n == 126:
        n = struct.unpack(">H", le_exato(conn, 2))[0]
    elif n == 127:
        n = struct.unpack(">Q", le_exato(conn, 8))[0]
    if n > MAX_QUADRO:
        raise ConnectionError("quadro grande demais")
    chave = le_exato(conn, 4) if mascarado else b""
    dados = le_exato(conn, n) if n else b""
    if mascarado and dados:
        dados = bytes(b ^ chave[i & 3] for i, b in enumerate(dados))
    return opcode, dados


def monta_quadro(opcode, carga):
    n = len(carga)
    if n < 126:
        cab = struct.pack(">BB", 0x80 | opcode, n)
    elif n < 65536:
        cab = struct.pack(">BBH", 0x80 | opcode, 126, n)
    else:
        cab = struct.pack(">BBQ", 0x80 | opcode, 127, n)
    return cab + carga


def manda_texto(estado, texto):
    quadro = monta_quadro(0x1, texto.encode("utf-8"))
    try:
        with estado["lock"]:
            estado["conn"].sendall(quadro)
    except Exception:
        try:
            estado["conn"].close()
        except Exception:
            pass


def manda_json(estado, obj):
    manda_texto(estado, json.dumps(obj, ensure_ascii=False))


def aperto_de_mao(conn, cab):
    chave = cab.get("sec-websocket-key", "").encode("latin-1")
    aceite = base64.b64encode(hashlib.sha1(chave + GUID).digest()).decode("ascii")
    resposta = (
        "HTTP/1.1 101 Switching Protocols\r\n"
        "Upgrade: websocket\r\n"
        "Connection: Upgrade\r\n"
        "Sec-WebSocket-Accept: " + aceite + "\r\n\r\n"
    )
    conn.sendall(resposta.encode("ascii"))


# --------------------------------------------------------------------------
# protocolo do jogo
# --------------------------------------------------------------------------
def trata(estado, m):
    t = m.get("t")

    if t == "ola":
        pid = str(m.get("id") or "")[:64]
        if not pid:
            pid = "anon-%x" % id(estado)
        estado["id"] = pid
        with trava:
            p = perfis.get(pid)
            if not isinstance(p, dict):
                p = perfil_novo(m.get("cor"))
                perfis[pid] = p
                grava()
            board = monta_board()
        manda_json(estado, {"t": "bemvindo", "id": pid, "perfil": p, "board": board})
        print("  entrou: %s (%s)" % ((p.get("nick") or "sem apelido"), pid[:10]))

    elif t == "pos":
        d = m.get("d")
        if isinstance(d, dict):
            estado["pos"] = d

    elif t == "salva":
        pid = estado.get("id")
        if not pid:
            return
        with trava:
            p = perfis.get(pid)
            if not isinstance(p, dict):
                p = perfil_novo(m.get("cor"))
                perfis[pid] = p
            if isinstance(m.get("nick"), str):
                p["nick"] = m["nick"][:14]
            if isinstance(m.get("coins"), (int, float)):
                p["coins"] = m["coins"]
            if isinstance(m.get("up"), dict):
                p["up"] = m["up"]
            if isinstance(m.get("best"), dict):
                p["best"] = m["best"]
            if isinstance(m.get("corridas"), (int, float)):
                p["corridas"] = m["corridas"]
            if isinstance(m.get("cor"), str):
                p["cor"] = m["cor"][:9]
            grava()
            board = monta_board()
            alvos = list(clientes.values())
        texto = json.dumps({"t": "board", "rows": board}, ensure_ascii=False)
        for e in alvos:
            manda_texto(e, texto)


def laco_ws(conn):
    estado = {"conn": conn, "id": None, "pos": None, "lock": threading.Lock()}
    with trava:
        clientes[conn] = estado
    while True:
        opcode, dados = le_quadro(conn)
        if opcode == 0x8:                       # close
            break
        if opcode == 0x9:                       # ping -> pong
            try:
                with estado["lock"]:
                    conn.sendall(monta_quadro(0xA, dados))
            except Exception:
                break
            continue
        if opcode != 0x1:
            continue
        try:
            m = json.loads(dados.decode("utf-8"))
        except Exception:
            continue
        if isinstance(m, dict):
            trata(estado, m)


def tique():
    """Manda a posicao de todo mundo, 15 vezes por segundo."""
    while True:
        time.sleep(TICK)
        with trava:
            alvos = list(clientes.values())
        pacote = [{"id": e["id"], "d": e["pos"]} for e in alvos if e.get("id") and e.get("pos")]
        if not pacote:
            continue
        texto = json.dumps({"t": "estado", "peers": pacote}, ensure_ascii=False)
        for e in alvos:
            manda_texto(e, texto)


# --------------------------------------------------------------------------
# HTTP
# --------------------------------------------------------------------------
def responde(conn, codigo, tipo, corpo):
    cab = (
        "HTTP/1.1 %s\r\n"
        "Content-Type: %s\r\n"
        "Content-Length: %d\r\n"
        "Cache-Control: no-store\r\n"
        "Connection: close\r\n\r\n" % (codigo, tipo, len(corpo))
    )
    conn.sendall(cab.encode("utf-8") + corpo)


def serve_http(conn, caminho):
    rota = caminho.split("?")[0]
    if rota in ("/", "/index.html", "/jogo.html"):
        try:
            with open(HTML, "rb") as f:
                corpo = f.read()
        except FileNotFoundError:
            responde(conn, "500 Internal Server Error", "text/plain; charset=utf-8",
                     "jogo.html nao foi encontrado ao lado do servidor.py".encode("utf-8"))
            return
        responde(conn, "200 OK", "text/html; charset=utf-8", corpo)
    else:
        responde(conn, "404 Not Found", "text/plain; charset=utf-8", b"nao existe")


def desconecta(conn):
    with trava:
        estado = clientes.pop(conn, None)
    if estado and estado.get("id"):
        print("  saiu: %s" % estado["id"][:10])


def atende(conn):
    conn.settimeout(300)
    try:
        req = b""
        while b"\r\n\r\n" not in req:
            pedaco = conn.recv(4096)
            if not pedaco:
                return
            req += pedaco
            if len(req) > 65536:
                return
        cabecalho = req.split(b"\r\n\r\n", 1)[0].decode("latin-1")
        linhas = cabecalho.split("\r\n")
        partes = linhas[0].split(" ")
        caminho = partes[1] if len(partes) > 1 else "/"
        cab = {}
        for linha in linhas[1:]:
            chave, _, valor = linha.partition(":")
            cab[chave.strip().lower()] = valor.strip()

        if caminho.split("?")[0] == "/ws" and "websocket" in cab.get("upgrade", "").lower():
            aperto_de_mao(conn, cab)
            conn.settimeout(None)
            laco_ws(conn)
        else:
            serve_http(conn, caminho)
    except Exception:
        pass
    finally:
        desconecta(conn)
        try:
            conn.close()
        except Exception:
            pass


# --------------------------------------------------------------------------
def meu_ip():
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        s.connect(("10.255.255.255", 1))
        return s.getsockname()[0]
    except Exception:
        return "127.0.0.1"
    finally:
        s.close()


def abre_porta():
    for porta in range(PORTA_INICIAL, PORTA_INICIAL + 12):
        srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        try:
            srv.bind(("0.0.0.0", porta))
            srv.listen(64)
            return srv, porta
        except OSError:
            srv.close()
    raise SystemExit("Nenhuma porta livre entre %d e %d." % (PORTA_INICIAL, PORTA_INICIAL + 11))


def main():
    if not os.path.exists(HTML):
        print("ERRO: o arquivo jogo.html precisa estar na mesma pasta que o servidor.py.")
        input("Enter para fechar...")
        return

    carrega()
    srv, porta = abre_porta()
    ip = meu_ip()
    endereco = "http://%s:%d" % (ip, porta)

    print("")
    print("  RACHA DE CADEIRAS - servidor no ar")
    print("  " + "-" * 46)
    print("  Voce joga em:      http://localhost:%d" % porta)
    print("  A galera abre em:  %s" % endereco)
    print("")
    print("  Manda esse segundo endereco no grupo. Eles so colam no navegador.")
    print("  Se o Windows pedir, clique em PERMITIR ACESSO em redes privadas.")
    print("  Jogadores salvos em: jogadores.json")
    print("  Feche esta janela para desligar o servidor.")
    print("")

    threading.Thread(target=tique, daemon=True).start()

    if "--sem-navegador" not in sys.argv:
        try:
            webbrowser.open("http://localhost:%d" % porta)
        except Exception:
            pass

    try:
        while True:
            conn, _ = srv.accept()
            threading.Thread(target=atende, args=(conn,), daemon=True).start()
    except KeyboardInterrupt:
        print("\n  desligando.")
    finally:
        srv.close()


if __name__ == "__main__":
    main()
