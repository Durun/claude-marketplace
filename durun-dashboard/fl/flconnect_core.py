# durun-dashboard の FL Connect の中身。入口の device_FLConnect.py が読み込み、reload の要求で読み直す。
# 受けるのは 5 つ：状態を読む（summary、read、params、changed）、起動したときの設定を読む（info）、AI が書いた Python を動かす（exec）、プロジェクトを上書き保存する（save）、echo。
# イベントの番号（REC）を受ける関数は、無い番号を渡すと FL を落とすので、1 つずつ確かめた番号（REC_OFFSETS）だけを引く。
# 読んだ値は生のまま返す。比べる、まとめる、決めるといった判断はダッシュボードのサーバーでする。

import ast
import builtins
import json
import math
import time

import arrangement
import channels
import device
import general
import midi
import mixer
import patterns
import playlist
import plugins
import transport
import ui
import utils

try:
    import traceback
except ImportError:
    traceback = None

VERSION = "0.5.0"
EXEC_LIMIT_SEC = 2.0
MAX_EXEC_LIMIT_SEC = 10.0
# 応答は SysEx で分けて運ぶ。print の出力と result が大きすぎると FL の主スレッドが長く止まるので、ここで切る。
MAX_OUTPUT_CHARS = 60000
# ミキサーのトラックごとのエフェクトのスロットの数。
SLOTS = 10
COLOR_MASK = 0xFFFFFF
MAX_PARAMS = 200

# チャンネルのつまみを読むイベントの番号（チャンネルの番号の先頭からの位置）。FL 26.1.7 の Sampler のチャンネルで、1 つずつ読んで落ちなかったもの（2026-11-01）。
# イベントの番号を受ける関数は、無い番号を渡すと FL を落とすので、ここに無い番号は引かない。
# 基本のつまみ（midi.py の REC_Chan_*）のうち、音量、パン、ミュート、送り先のミキサーは、ほかの読み方（_channels）で読むので外す。
# Envelope は 5 つ（Panning、Volume、Filter cutoff、Filter resonance、Pitch。0x1000 から 0x100 ずつ）で、つまみは 0〜12 番。13 番（REC_Chan_Env_Hole）は引くと落ちた。
REC_OFFSETS = (2, 3, 4, 5, 6, 9, 10, 11, 12, 13, 14, 16, 17, 18, 19, 20) + tuple(0x1000 + k * 0x100 + p for k in range(5) for p in range(13))
# REC_OFFSETS を確かめた種類のチャンネル（channels.getChannelType の CT_Sampler）。ほかの種類は、番号の並びが違うかもしれないので読まない。
REC_CHANNEL_TYPES = (0,)

_FL_MODULES = {
    "arrangement": arrangement, "channels": channels, "device": device, "general": general, "mixer": mixer,
    "patterns": patterns, "playlist": playlist, "plugins": plugins, "transport": transport, "ui": ui,
}
# 値を変える関数を持たないので、書き込みを止めている間もそのまま渡すモジュール。
_PURE_MODULES = {"midi": midi, "utils": utils, "json": json, "time": time, "math": math}
# get、is、has で始まらないが、値を読むだけの関数。
_READ_NAMES = {"channelCount", "trackCount", "patternCount", "patternMax", "channelNumber", "selectedChannel", "trackNumber", "patternNumber"}

_WINDOWS = (("mixer", "widMixer", 0), ("channels", "widChannelRack", 1), ("playlist", "widPlaylist", 2), ("pianoroll", "widPianoRoll", 3), ("browser", "widBrowser", 4))


def _error_text(e):
    return traceback.format_exc() if traceback else "%s: %s" % (type(e).__name__, e)


def _is_read(name):
    return name.startswith(("get", "is", "has")) or name.endswith("Count") or name in _READ_NAMES


class _ReadOnly:
    """書き込みを止めている間、AI のコードに渡すモジュールの代わり。値を読む関数だけを通し、ほかは呼ぶと例外にする。"""

    def __init__(self, name, module):
        self._name = name
        self._module = module

    def __getattr__(self, attr):
        value = getattr(self._module, attr)
        if not callable(value) or _is_read(attr):
            return value
        full = "%s.%s" % (self._name, attr)

        def blocked(*args, **kwargs):
            raise PermissionError("FL Connect のウィンドウで書き込みを止めているので、%s は呼べません" % full)

        return blocked


class _Capture:
    """exec のあいだ sys.stdout の代わりに置き、print の出力を溜める。"""

    def __init__(self):
        self.parts = []
        self.size = 0

    def write(self, s):
        s = str(s)
        if self.size < MAX_OUTPUT_CHARS:
            self.parts.append(s)
        self.size += len(s)
        return len(s)

    def flush(self):
        pass

    def text(self):
        joined = "".join(self.parts)
        if self.size <= MAX_OUTPUT_CHARS:
            return joined
        return joined[:MAX_OUTPUT_CHARS] + "\n…（出力が長いので、%d 文字のうち先頭の %d 文字だけを返した）" % (self.size, MAX_OUTPUT_CHARS)


class _Guard(ast.NodeTransformer):
    """
    FL の囲いは sys.settrace を塞ぐので、時間切れはコードの書き換えで入れる。
    ループと関数の先頭で時刻を確かめる呼び出しを差し込み、内包表記は回す対象を包む。
    """

    @staticmethod
    def _check():
        return ast.Expr(ast.Call(ast.Name("__fl_check__", ast.Load()), [], []))

    def _guard_body(self, node):
        self.generic_visit(node)
        node.body.insert(0, self._check())
        return node

    visit_While = _guard_body
    visit_For = _guard_body
    visit_FunctionDef = _guard_body

    def visit_comprehension(self, node):
        self.generic_visit(node)
        node.iter = ast.Call(ast.Name("__fl_iter__", ast.Load()), [node.iter], [])
        return node


def _compile(code):
    tree = _Guard().visit(ast.parse(code, "<fl_python>", "exec"))
    ast.fix_missing_locations(tree)
    return compile(tree, "<fl_python>", "exec")


def _env(readonly):
    modules = dict(_PURE_MODULES)
    modules.update({name: (_ReadOnly(name, m) if readonly else m) for name, m in _FL_MODULES.items()})
    env = dict(modules)
    if readonly:
        # コードの中で import し直しても、読むだけの代わりが届くようにする。
        real_import = builtins.__import__

        def guarded_import(name, globals=None, locals=None, fromlist=(), level=0):
            if level == 0 and name in modules:
                return modules[name]
            return real_import(name, globals, locals, fromlist, level)

        guarded = dict(vars(builtins))
        guarded["__import__"] = guarded_import
        env["__builtins__"] = guarded
    return env


def _jsonable(value):
    """result を JSON にできる形にし、大きすぎれば文字に縮める。"""
    try:
        text = json.dumps(value, ensure_ascii=False, default=str)
    except Exception:
        text = json.dumps(str(value), ensure_ascii=False)
    if len(text) <= MAX_OUTPUT_CHARS:
        return json.loads(text)
    return "（result が長いので、%d 文字のうち先頭の %d 文字だけを返した）%s" % (len(text), MAX_OUTPUT_CHARS, text[:MAX_OUTPUT_CHARS])


def exec_code(code, limit=EXEC_LIMIT_SEC, readonly=False):
    """AI のコードを動かし、print の出力と result の値を返す。Python の行のループは limit 秒で止まるが、組み込み関数や FL の API の 1 回の長い呼び出しは止められない。"""
    import sys

    limit = min(float(limit), MAX_EXEC_LIMIT_SEC)
    started = time.perf_counter()
    deadline = started + limit

    def check():
        if time.perf_counter() > deadline:
            raise TimeoutError("FL Connect: %.1f 秒を超えたので止めた" % limit)

    def guarded_iter(iterable):
        for item in iterable:
            check()
            yield item

    env = _env(readonly)
    env.update({"result": None, "__fl_check__": check, "__fl_iter__": guarded_iter})
    out = _Capture()
    saved_out = sys.stdout
    ok, error = True, None
    try:
        compiled = _compile(code)
        sys.stdout = out
        try:
            exec(compiled, env)
        finally:
            sys.stdout = saved_out
    except BaseException as e:
        ok = False
        error = _error_text(e)
    return {"ok": ok, "error": error, "stdout": out.text(), "result": _jsonable(env.get("result")), "ms": round((time.perf_counter() - started) * 1000, 1)}


def _finite(value, digits):
    """JSON に無限大を書くと JavaScript で読めないので、無限大と NaN は None にする。"""
    value = float(value)
    return round(value, digits) if math.isfinite(value) else None


def _plugin(index, slot=-1):
    try:
        if plugins.isValid(index, slot):
            return plugins.getPluginName(index, slot)
    except Exception:
        pass
    return None


def _focus():
    for name, attr, default in _WINDOWS:
        if ui.getFocused(getattr(midi, attr, default)):
            return [name, ui.getFocusedFormCaption()]
    return [None, ui.getFocusedFormCaption()]


def _tempo():
    # FL 26.1.7 は、asInt を省いても BPM の 1000 倍の値を返す（140 BPM で 140000）。テンポは 10 から 999 BPM なので、1000 以上なら割る。
    tempo = float(mixer.getCurrentTempo())
    return _finite(tempo / 1000 if tempo >= 1000 else tempo, 3)


# 前面のプラグインと、そのパラメーターの覚えた値。人間はプラグインのウィンドウでつまみを回すので、前面のプラグインだけを要約のたびに比べる。
# 名前のあるパラメーターだけを比べる（VST は 4240 個のうち、名前のあるものが 600 ほど。名前と値は全部で 3 ミリ秒）。
# 値の文字はプラグインに 1 つずつ尋ねるので遅い（1 つ 0.15 ミリ秒）。変わったものだけを読み、次に変わったときの前の文字にする。
_watched = None


def _focused_plugin():
    """
    前面のプラグイン（form, 名前, index, slot, useGlobalIndex）。無ければ None。
    ui.getFocusedFormID は、エフェクトなら ((トラック << 6) + スロット) << 16 を、音源ならチャンネルの番号を返した（FL 26.1.7）。
    """
    name = ui.getFocusedPluginName()
    if not name:
        return None
    form = ui.getFocusedFormID()
    if form >= 1 << 16:
        index, slot, glob = form >> 22, (form >> 16) & 0x3F, False
    else:
        index, slot, glob = form, -1, True
    if index < 0 or not plugins.isValid(index, slot, glob) or plugins.getPluginName(index, slot, glob) != name:
        return None
    return form, name, index, slot, glob


def _plugin_changes():
    """前面のプラグインで、前に読んでから変わったパラメーター。読めなければ（読む途中でプラグインを外したなど）、覚えた値を捨てて、要約は返す。"""
    global _watched
    try:
        return _read_plugin_changes()
    except Exception:
        _watched = None
        return None


def _read_plugin_changes():
    """前面が別のプラグインになったら、比べずに覚え直す。"""
    global _watched
    focus = _focused_plugin()
    if focus is None:
        _watched = None
        return None
    form, name, index, slot, glob = focus
    if _watched is None or _watched["key"] != (form, name):
        names = {}
        for p in range(plugins.getParamCount(index, slot, glob)):
            param = plugins.getParamName(p, index, slot, glob)
            if param:
                names[p] = param
        _watched = {"key": (form, name), "names": names, "values": {p: plugins.getParamValue(p, index, slot, glob) for p in names}, "texts": {}}
        return None
    changes = []
    for p, before in _watched["values"].items():
        after = plugins.getParamValue(p, index, slot, glob)
        if after == before:
            continue
        text = plugins.getParamValueString(p, index, slot, glob)
        changes.append([p, _watched["names"][p], before, after, _watched["texts"].get(p, ""), text])
        _watched["values"][p] = after
        _watched["texts"][p] = text
    return {"plugin": name, "index": index, "slot": slot, "changes": changes} if changes else None


def summary():
    """
    決まった間隔で問い合わせる、安い要約。元に戻す履歴の数と位置で、どこかが変わったことに気付く。
    plugin は、前面のプラグインで前に読んでから変わったパラメーター（[番号, 名前, 前の値, 後の値, 前の値の文字, 後の値の文字]）。
    """
    return {
        "plugin": _plugin_changes(),
        "ok": True,
        "playing": bool(transport.isPlaying()),
        "recording": bool(transport.isRecording()),
        "tempo": _tempo(),
        "pos": transport.getSongPosHint(),
        "song": transport.getLoopMode() == 1,
        "channel": channels.selectedChannel(),
        "mixer": mixer.trackNumber(),
        "pattern": patterns.patternNumber(),
        "focus": _focus(),
        "undo": [general.getUndoHistoryCount(), general.getUndoHistoryLast(), general.getUndoLevelHint()],
        "counts": [channels.channelCount(), patterns.patternCount(), mixer.trackCount(), playlist.trackCount()],
        "changed": general.getChangedFlag(),
    }


def _channels():
    count = channels.channelCount()
    # isChannelSolo は、ほかのチャンネルが鳴らないときに True を返すので、チャンネルが 1 つだけだと、ソロを押していなくても True になる。
    return [
        {
            "i": i, "name": channels.getChannelName(i), "color": channels.getChannelColor(i) & COLOR_MASK,
            "vol": _finite(channels.getChannelVolume(i), 4), "db": _finite(channels.getChannelVolume(i, True), 1),
            "pan": _finite(channels.getChannelPan(i), 4), "mute": bool(channels.isChannelMuted(i)), "solo": count > 1 and bool(channels.isChannelSolo(i)),
            "mixer": channels.getTargetFxTrack(i), "type": channels.getChannelType(i), "plugin": _plugin(i), "sel": bool(channels.isChannelSelected(i)),
        }
        for i in range(count)
    ]


_EQ_GETTERS = (mixer.getEqGain, mixer.getEqFrequency, mixer.getEqBandwidth)


def _mixer(slots):
    """ミキサーのトラック。eq は、帯ごとの Gain、Frequency、Bandwidth（0 から 1）を順に並べたもの。"""
    items = []
    bands = mixer.getEqBandCount()
    for i in range(mixer.trackCount()):
        item = {
            "i": i, "name": mixer.getTrackName(i), "color": mixer.getTrackColor(i) & COLOR_MASK,
            "vol": _finite(mixer.getTrackVolume(i), 4), "db": _finite(mixer.getTrackVolume(i, 1), 1), "pan": _finite(mixer.getTrackPan(i), 4),
            "mute": bool(mixer.isTrackMuted(i)), "solo": bool(mixer.isTrackSolo(i)),
            "stereo": _finite(mixer.getTrackStereoSep(i), 6), "eq": [_finite(get(i, b), 6) for b in range(bands) for get in _EQ_GETTERS],
            "enabled": bool(mixer.isTrackEnabled(i)), "polarity": bool(mixer.isTrackRevPolarity(i)), "swap": bool(mixer.isTrackSwapChannels(i)),
            "armed": bool(mixer.isTrackArmed(i)), "fx": bool(mixer.isTrackSlotsEnabled(i)),
        }
        if slots:
            item["slots"] = [_plugin(i, s) for s in range(SLOTS)]
        items.append(item)
    return items


def _sends(tracks):
    """tracks の送り先と、送り先ごとの送りの量（0 から 1）。"""
    count = mixer.trackCount()
    sends, levels = {}, {}
    for i in tracks:
        if not 0 <= i < count:
            continue
        dests = [d for d in range(count) if d != i and mixer.getRouteSendActive(i, d)]
        sends[str(i)] = dests
        levels[str(i)] = [_finite(mixer.getRouteToLevel(i, d), 6) for d in dests]
    return sends, levels


def _rec(names):
    """
    チャンネルのつまみを、REC_OFFSETS のイベントの番号で読む。値は FL の生の値（0 から 2^30）と、FL が付けた値の文字。
    names なら、FL が付けたつまみの名前も足す。名前はチャンネルの名前を含むので、チャンネルを足すか名前を変えるまで変わらない。
    """
    out = {}
    for i in range(channels.channelCount()):
        if channels.getChannelType(i) not in REC_CHANNEL_TYPES:
            continue
        base = channels.getRecEventId(i)
        values = [[mixer.getEventValue(base + o), device.getLinkedValueString(base + o)] for o in REC_OFFSETS]
        if names:
            for value, o in zip(values, REC_OFFSETS):
                value.append(mixer.getEventIDName(base + o))
        out[str(i)] = values
    return out


def _playlist():
    """プレイリストのトラックは 500 あるので、末尾の触っていないトラック（名前が既定で、色が最後のトラックと同じ）は返さない。"""
    count = playlist.trackCount()
    items = [
        {"i": i, "name": playlist.getTrackName(i), "color": playlist.getTrackColor(i) & COLOR_MASK, "mute": bool(playlist.isTrackMuted(i)), "solo": bool(playlist.isTrackSolo(i))}
        for i in range(1, count + 1)
    ]
    plain = items[-1]["color"] if items else None
    while items and items[-1]["name"] == "Track %d" % items[-1]["i"] and items[-1]["color"] == plain and not items[-1]["mute"] and not items[-1]["solo"]:
        items.pop()
    return {"items": items, "count": count}


def _patterns():
    return [
        {"i": i, "name": patterns.getPatternName(i), "color": patterns.getPatternColor(i) & COLOR_MASK, "length": patterns.getPatternLength(i)}
        for i in range(1, patterns.patternCount() + 1)
    ]


def read(req):
    """
    sections に挙げた範囲を読む。slots でミキサーのスロットのプラグインも、sends に挙げたトラックの送り先と送りの量も読む。
    rec でチャンネルのつまみも読み（rec_names なら名前も）、rec_offsets に番号の並びを添える。
    """
    sections = req.get("sections")
    if sections is None:
        sections = ["channels", "mixer", "playlist", "patterns"]
    out = {"ok": True, "summary": summary()}
    if "channels" in sections:
        out["channels"] = _channels()
    if "mixer" in sections:
        out["mixer"] = _mixer(bool(req.get("slots")))
    if "playlist" in sections:
        out["playlist"] = _playlist()
    if "patterns" in sections:
        out["patterns"] = _patterns()
    if req.get("sends"):
        out["sends"], out["levels"] = _sends(req["sends"])
    if req.get("rec"):
        out["rec"] = _rec(bool(req.get("rec_names")))
        out["rec_offsets"] = REC_OFFSETS
    return out


def params(req):
    """チャンネルかミキサーのスロットのプラグインのパラメーターを、名前の無いものを除いて読む。"""
    index = int(req.get("index", 0))
    slot = int(req.get("slot", -1))
    if not plugins.isValid(index, slot):
        return {"ok": False, "error": "プラグインがありません（index %d、slot %d）" % (index, slot)}
    start = int(req.get("start", 0))
    limit = min(int(req.get("limit", MAX_PARAMS)), MAX_PARAMS)
    count = plugins.getParamCount(index, slot)
    items = []
    p = start
    while p < count and len(items) < limit:
        name = plugins.getParamName(p, index, slot)
        if name:
            items.append({"p": p, "name": name, "value": _finite(plugins.getParamValue(p, index, slot), 4), "text": plugins.getParamValueString(p, index, slot)})
        p += 1
    return {"ok": True, "plugin": plugins.getPluginName(index, slot), "count": count, "next": p if p < count else None, "items": items}


def info():
    """FL を起動したときの設定のうち、ダッシュボードが知りたいもの。デバッグのポートを開く設定で起動したかを、FL の環境変数で見る。"""
    try:
        import os
        webview2 = os.environ.get("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS")
    except Exception:
        webview2 = None
    return {"ok": True, "webview2": webview2}


def save():
    """
    プロジェクトを上書き保存する（Ctrl+S と同じ）。名前の無いプロジェクトでは、FL が保存のダイアログを出して止まるので、呼ぶ側で避ける。
    status は globalTransport が返した値。changed は呼んだあとの、保存していない変更の印（0 なら保存した）。
    """
    status = transport.globalTransport(midi.FPT_Save, 1)
    return {"ok": True, "status": status, "changed": general.getChangedFlag()}


def handle(req):
    op = req.get("op")
    if op == "info":
        return info()
    if op == "save":
        return save()
    if op == "changed":
        # 保存が終わったかを確かめる、安い問い合わせ。summary は前面のプラグインの値を比べ直すので使わない。
        return {"ok": True, "changed": general.getChangedFlag()}
    if op == "exec":
        return exec_code(req.get("code", ""), req.get("limit", EXEC_LIMIT_SEC), bool(req.get("readonly")))
    if op == "summary":
        return summary()
    if op == "read":
        return read(req)
    if op == "params":
        return params(req)
    if op == "echo":
        return {"ok": True, "echo": req.get("data")}
    return {"ok": False, "error": "知らない要求です: %s" % op}
