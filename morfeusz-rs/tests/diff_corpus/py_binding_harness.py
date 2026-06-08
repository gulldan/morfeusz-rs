"""Run the SAME morfeusz2 Python API under whichever binding is on sys.path.

Modes:
  dump   <corpus>  -> deterministic dump of high-level analyse() tuples (parity)
  dumpg  <lemmas>  -> deterministic dump of generate() tuples
  dag    <corpus>  -> dump expand_dag analyse (exercises DAG expansion)
  bench  <corpus>  -> time analyse() over the corpus (throughput)
  api              -> print the exposed API surface (classes/callables)
"""
import sys, time
import morfeusz2

DICT = dict(dict_name="sgjp", dict_path="/tmp/bench")


def lines(path):
    with open(path, encoding="utf-8") as f:
        return [l.rstrip("\n") for l in f]


def mode_dump(corpus):
    # Stream output (don't accumulate) so peak RSS reflects the binding, not the
    # harness holding the whole dump in memory.
    m = morfeusz2.Morfeusz(**DICT, analyse=True, generate=False)
    w = sys.stdout
    for line in lines(corpus):
        for t in m.analyse(line):
            w.write(repr(t)); w.write("\n")


def mode_dumpg(corpus):
    m = morfeusz2.Morfeusz(**DICT, analyse=False, generate=True)
    w = sys.stdout
    for line in lines(corpus):
        for t in m.generate(line):
            w.write(repr(t)); w.write("\n")


def mode_dag(corpus):
    m = morfeusz2.Morfeusz(**DICT, analyse=True, generate=False, expand_dag=True)
    w = sys.stdout
    for line in lines(corpus):
        w.write(repr(m.analyse(line))); w.write("\n")


def mode_bench(corpus):
    m = morfeusz2.Morfeusz(**DICT, analyse=True, generate=False)
    data = lines(corpus)
    t0 = time.perf_counter()
    n = 0
    for line in data:
        n += len(m.analyse(line))
    dt = time.perf_counter() - t0
    sys.stderr.write(f"binding={_binding()} lines={len(data)} interps={n} "
                     f"time={dt:.3f}s rate={len(data)/dt:,.0f} lines/s\n")


def mode_api():
    surface = {}
    for cls in ("Morfeusz", "_Morfeusz", "MorphInterpretation", "ResultsIterator", "IdResolver"):
        obj = getattr(morfeusz2, cls, None)
        if obj is None:
            surface[cls] = None
            continue
        members = sorted(n for n in dir(obj) if not n.startswith("__"))
        surface[cls] = members
    consts = sorted(n for n in dir(morfeusz2)
                    if n.isupper() or n in ("__version__", "__copyright__", "GENDERS"))
    print("BINDING:", _binding())
    print("MODULE-CONSTS:", consts)
    for cls, members in surface.items():
        print(f"{cls}:", members)


def _binding():
    f = getattr(morfeusz2, "__file__", "") or ""
    return "rust" if "site-packages" in f and "cppy" not in f else "cpp"


if __name__ == "__main__":
    {"dump": mode_dump, "dumpg": mode_dumpg, "dag": mode_dag,
     "bench": mode_bench, "api": lambda *_: mode_api()}[sys.argv[1]](*sys.argv[2:])
