# name=FL Connect
# supportedDevices=FL Connect (A)
# durun-dashboard の FL Connect の入口。ダッシュボードの中継と、ループバックの MIDI ポートの SysEx でやりとりする。
# メッセージは F0 7D 'F' 'C' <向き> <番号> <何通目> <全部で何通> <JSON> F7。番号などの数は 7 ビットずつの 2 バイト。
# JSON は ASCII の文字だけで書く。SysEx のデータは 0x80 未満でなければならないため。
# 要求の中身は flconnect_core.py が処理する。reload の要求でそれを読み直すので、入口を変えない限り FL の操作は要らない。
# FL の主スレッドで動くので、スレッドを作らず、sleep もしない。

import importlib
import json
import sys
import time

import device
import general
import ui

try:
    import traceback
except ImportError:
    traceback = None


def _error_text(e):
    return traceback.format_exc() if traceback else "%s: %s" % (type(e).__name__, e)


try:
    import flconnect_core as core
    _core_error = None
except Exception as e:
    core = None
    _core_error = _error_text(e)

VERSION = "0.1.0"
HEADER = (0x7D, 0x46, 0x43)
REQUEST, RESPONSE, HEARTBEAT, NOTICE = 1, 2, 3, 4
# Windows の MIDI は長い SysEx を捨てることがあるので、1 通の中身はこの大きさまでにする。
CHUNK = 240
HEARTBEAT_SEC = 0.5
# 変化の印は、前に知らせてからこの時間がたつまで溜め、まとめて知らせる。
NOTICE_MIN_SEC = 0.1
MAX_DIRTY = 200

_inbox = {}
_dirty = []
_dropped = 0
_last_notice = 0.0
_last_heartbeat = 0.0


def _u14(n):
    return [(n >> 7) & 0x7F, n & 0x7F]


def _send(direction, msg_id, payload):
    body = json.dumps(payload, ensure_ascii=True, separators=(",", ":"), default=str).encode("ascii")
    parts = [body[i:i + CHUNK] for i in range(0, len(body), CHUNK)] or [b""]
    try:
        for idx, part in enumerate(parts):
            device.midiOutSysex(bytes([0xF0, *HEADER, direction, *_u14(msg_id), *_u14(idx), *_u14(len(parts))]) + part + bytes([0xF7]))
    except Exception as e:
        # 出力のポートを割り当てていないときなど。スクリプトは止めない。
        print("[FL Connect] SysEx を送れません: %s" % e)


def _reload():
    global core, _core_error
    try:
        core = importlib.reload(core) if core else importlib.import_module("flconnect_core")
        _core_error = None
    except Exception as e:
        _core_error = _error_text(e)
    return {"ok": _core_error is None, "core": getattr(core, "VERSION", None), "error": _core_error}


def _hello():
    return {"ok": True, "script": VERSION, "core": getattr(core, "VERSION", None), "core_error": _core_error,
            "fl": ui.getVersion(), "api": general.getVersion(), "python": sys.version.split()[0]}


def _handle(req):
    op = req.get("op")
    if op == "hello":
        return _hello()
    if op == "reload":
        return _reload()
    if core is None:
        return {"ok": False, "error": "flconnect_core を読めません: %s" % _core_error}
    return core.handle(req)


def _receive(data):
    """FL Connect の SysEx なら処理して True を返す。要求がそろったら応答を返す。"""
    if len(data) < 12 or data[0] != 0xF0 or tuple(data[1:4]) != HEADER:
        return False
    direction = data[4]
    msg_id = (data[5] << 7) | data[6]
    idx = (data[7] << 7) | data[8]
    total = (data[9] << 7) | data[10]
    body = bytes(data[11:-1]) if data[-1] == 0xF7 else bytes(data[11:])
    if direction != REQUEST:
        return True
    parts = _inbox.setdefault(msg_id, {})
    parts[idx] = body
    if len(parts) < total:
        return True
    del _inbox[msg_id]
    try:
        reply = _handle(json.loads(b"".join(parts[i] for i in range(total)).decode("ascii")))
    except Exception as e:
        reply = {"ok": False, "error": _error_text(e)}
    _send(RESPONSE, msg_id, reply)
    return True


def _take_dirty():
    global _dirty, _dropped
    items, dropped = _dirty, _dropped
    _dirty, _dropped = [], 0
    return {"items": items, "dropped": dropped}


def _flush(now):
    global _last_notice
    _last_notice = now
    if _dirty or _dropped:
        _send(NOTICE, 0, {"dirty": _take_dirty()})


def _mark(*item):
    """変化の印だけを溜める。値は読まない。前に知らせてから NOTICE_MIN_SEC たっていれば、すぐ知らせる。"""
    global _dropped
    if len(_dirty) < MAX_DIRTY:
        _dirty.append(list(item))
    else:
        _dropped += 1
    now = time.perf_counter()
    if now - _last_notice >= NOTICE_MIN_SEC:
        _flush(now)


def OnInit():
    # AI のコードが例外で抜けて sys.stdout を差し替えたまま残ったときに備え、読み直すたびに戻す。
    if sys.__stdout__ is not None and type(sys.stdout).__name__ == "_Capture":
        sys.stdout = sys.__stdout__
    _send(NOTICE, 0, {"hello": _hello()})


def OnDeInit():
    _send(NOTICE, 0, {"bye": True})


def OnIdle():
    global _last_heartbeat
    now = time.perf_counter()
    if (_dirty or _dropped) and now - _last_notice >= NOTICE_MIN_SEC:
        _flush(now)
    if now - _last_heartbeat >= HEARTBEAT_SEC:
        _last_heartbeat = now
        _send(HEARTBEAT, 0, {})


def OnSysEx(event):
    if _receive(bytes(event.sysex)):
        event.handled = True


def OnMidiMsg(event):
    # 版によっては SysEx がこちらに届くので、ここでも受ける。
    try:
        sysex = event.sysex
    except Exception:
        sysex = None
    if sysex and _receive(bytes(sysex)):
        event.handled = True


# 変化のコールバックの中では FL の値を変えない。変えると、その変更がまたコールバックを呼ぶ。
def OnRefresh(flags):
    _mark("refresh", flags)


def OnDirtyMixerTrack(index):
    _mark("mixer", index)


def OnDirtyChannel(index, flag=0):
    _mark("channel", index, flag)


def OnProjectLoad(status):
    _mark("project", status)
