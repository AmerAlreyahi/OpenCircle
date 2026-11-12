#!/usr/bin/env python3 -I
"""Check the store manifests against the published JSON schemas (vendored in schemas/).
Usage: validate.py [winget-dir ...]   (default: every packaging/stores/winget/*/ + scoop/opencircle.json)
Needs python3-jsonschema and PyYAML. Exit 1 on any error."""
import glob, json, os, sys, datetime
import jsonschema, yaml

here = os.path.dirname(os.path.abspath(__file__))
ok = True

def fail(msg):
    global ok
    ok = False
    print("FAIL", msg)

def plain(x):  # YAML dates -> text, as the schema expects strings
    if isinstance(x, dict): return {k: plain(v) for k, v in x.items()}
    if isinstance(x, list): return [plain(v) for v in x]
    if isinstance(x, (datetime.date, datetime.datetime)): return x.isoformat()
    return x

def check(doc, schema_file, name):
    schema = json.load(open(os.path.join(here, "schemas", schema_file)))
    errs = sorted(jsonschema.Draft7Validator(schema, format_checker=jsonschema.FormatChecker()).iter_errors(doc), key=str)
    for e in errs: fail(f"{name}: {'/'.join(map(str, e.path))}: {e.message}")
    if not errs: print("ok  ", name)

dirs = sys.argv[1:] or sorted(glob.glob(os.path.join(here, "winget", "*")))
for d in dirs:
    docs = {}
    for f in sorted(glob.glob(os.path.join(d, "*.yaml"))):
        doc = plain(yaml.safe_load(open(f)))
        docs[doc["ManifestType"]] = doc
        check(doc, {"version": "winget-version.1.28.0.json", "installer": "winget-installer.1.28.0.json",
                    "defaultLocale": "winget-defaultLocale.1.28.0.json"}[doc["ManifestType"]], os.path.basename(f))
        base = os.path.basename(f)
        pid, ver = doc["PackageIdentifier"], doc["PackageVersion"]
        want = {"version": f"{pid}.yaml", "installer": f"{pid}.installer.yaml",
                "defaultLocale": f"{pid}.locale.{doc.get('PackageLocale','')}.yaml"}[doc["ManifestType"]]
        if base != want: fail(f"{base}: file name must be {want}")
        if os.path.basename(d) != ver: fail(f"{base}: PackageVersion {ver} is not the folder name")
    if set(docs) != {"version", "installer", "defaultLocale"}: fail(f"{d}: need version + installer + defaultLocale")
    ids = {(x["PackageIdentifier"], x["PackageVersion"]) for x in docs.values()}
    if len(ids) != 1: fail(f"{d}: files disagree on id/version")
    if docs.get("version", {}).get("DefaultLocale") != docs.get("defaultLocale", {}).get("PackageLocale"):
        fail(f"{d}: DefaultLocale mismatch")

# Scoop: schema.json (draft-07) of ScoopInstaller/Scoop, plus the rules the bucket CI adds
sj = os.environ.get("SCOOP_JSON") or os.path.join(here, "scoop", "opencircle.json")
if (os.environ.get("SCOOP_JSON") or not sys.argv[1:]) and os.path.exists(sj):
    doc = json.load(open(sj))
    check(doc, "scoop-schema.json", "scoop/opencircle.json")
    u = doc["architecture"]["64bit"]["url"]
    if doc["version"] not in u: fail("scoop: url lacks the version")
    if "$version" not in doc["autoupdate"]["architecture"]["64bit"]["url"]: fail("scoop: autoupdate url lacks $version")
sys.exit(0 if ok else 1)
