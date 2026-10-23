/*
 * PS3_PATCHES="Skip Intro,Extended FOV": community patches, read from the
 * player's own RPCS3 patch file and applied to guest memory right after the
 * EBOOT is loaded, before the game runs. Nothing is shipped with the port:
 * the file is RPCS3's (its patch manager downloads it), the patches are their
 * authors' (credited in the log).
 *
 * Only the block of this EBOOT counts: RPCS3 keys patches by "PPU-<sha1>",
 * the SHA-1 of the ELF's program headers and LOAD segments (RPCS3's
 * ppu_load_exec), computed here the same way. A static recompilation runs the
 * game's code from the translation, so only patches to DATA take effect at
 * load time; inFAMOUS 2's three (Skip Intro: the splash movie's name string;
 * Extended FOV: a float; Disable Mesh Trimming: a table word) all are.
 *
 *   PS3_PATCHES=<name>[,<name>...]   patch names as in the file (case-insensitive)
 *   PS3_PATCH_YML=<path>             default: RPCS3's patches/patch.yml
 */
#include <CommonCrypto/CommonDigest.h>
#include <ctype.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <string>
#include <vector>

extern uint8_t* vm_base;

namespace {

bool ppu_hash(const char* elf_path, char out[41])
{
    FILE* f = fopen(elf_path, "rb");
    if (!f) return false;
    std::vector<uint8_t> d;
    { uint8_t buf[1 << 16]; size_t n;
      while ((n = fread(buf, 1, sizeof buf, f)) > 0) d.insert(d.end(), buf, buf + n); }
    fclose(f);
    auto be16 = [&](size_t o) { return (uint32_t)d[o] << 8 | d[o + 1]; };
    auto be32 = [&](size_t o) { return (uint32_t)be16(o) << 16 | be16(o + 2); };
    auto be64 = [&](size_t o) { return (uint64_t)be32(o) << 32 | be32(o + 4); };
    if (d.size() < 0x40 || memcmp(d.data(), "\x7f" "ELF", 4)) return false;
    const uint64_t phoff = be64(0x20);
    const uint32_t phentsize = be16(0x36), phnum = be16(0x38);
    CC_SHA1_CTX c;
    CC_SHA1_Init(&c);
    for (uint32_t i = 0; i < phnum; i++) {
        const size_t o = phoff + (size_t)i * phentsize;
        if (o + 56 > d.size()) return false;
        CC_SHA1_Update(&c, &d[o], 4);       /* p_type, big-endian as stored */
        CC_SHA1_Update(&c, &d[o + 4], 4);   /* p_flags */
        const uint64_t off = be64(o + 8), vaddr = be64(o + 16), filesz = be64(o + 32), memsz = be64(o + 40);
        if (be32(o) == 1 && memsz) {
            if (off + filesz > d.size()) return false;
            CC_SHA1_Update(&c, &d[o + 16], 8);   /* p_vaddr */
            CC_SHA1_Update(&c, &d[o + 40], 8);   /* p_memsz */
            CC_SHA1_Update(&c, &d[off], (CC_LONG)filesz);
            (void)vaddr;
        }
    }
    unsigned char md[CC_SHA1_DIGEST_LENGTH];
    CC_SHA1_Final(md, &c);
    for (int i = 0; i < CC_SHA1_DIGEST_LENGTH; i++) snprintf(out + 2 * i, 3, "%02x", md[i]);
    return true;
}

std::string lower(std::string s) { for (auto& ch : s) ch = (char)tolower((unsigned char)ch); return s; }
std::string trim(std::string s)
{
    size_t a = s.find_first_not_of(" \t\r\""), b = s.find_last_not_of(" \t\r\"");
    return a == std::string::npos ? std::string() : s.substr(a, b - a + 1);
}

bool apply_one(const std::string& type, uint32_t addr, const std::string& value)
{
    if (!vm_base) return false;
    uint8_t* p = vm_base + addr;
    auto put = [&](uint64_t v, int n) { for (int i = 0; i < n; i++) p[i] = (uint8_t)(v >> (8 * (n - 1 - i))); };
    if (type == "byte")  { put(strtoull(value.c_str(), nullptr, 0), 1); return true; }
    if (type == "be16")  { put(strtoull(value.c_str(), nullptr, 0), 2); return true; }
    if (type == "be32")  { put(strtoull(value.c_str(), nullptr, 0), 4); return true; }
    if (type == "be64")  { put(strtoull(value.c_str(), nullptr, 0), 8); return true; }
    if (type == "bef32") { const float f = strtof(value.c_str(), nullptr); uint32_t u; memcpy(&u, &f, 4); put(u, 4); return true; }
    if (type == "bef64") { const double f = strtod(value.c_str(), nullptr); uint64_t u; memcpy(&u, &f, 8); put(u, 8); return true; }
    return false;
}

} // namespace

extern "C" void infamous2_apply_patches(const char* elf_path)
{
    const char* want_env = getenv("PS3_PATCHES");
    if (!want_env || !*want_env) return;
    std::vector<std::string> want;
    { std::string s = want_env; size_t a = 0;
      while (a <= s.size()) { size_t b = s.find(',', a); if (b == std::string::npos) b = s.size();
                              const std::string n = lower(trim(s.substr(a, b - a))); if (!n.empty()) want.push_back(n); a = b + 1; } }
    std::string yml;
    if (const char* e = getenv("PS3_PATCH_YML")) yml = e;
    else if (const char* h = getenv("HOME")) yml = std::string(h) + "/Library/Application Support/rpcs3/patches/patch.yml";
    char hash[41] = {0};
    if (!ppu_hash(elf_path, hash)) { fprintf(stderr, "[patch] cannot hash %s; no patches\n", elf_path); return; }
    FILE* f = fopen(yml.c_str(), "r");
    if (!f) { fprintf(stderr, "[patch] no patch file at %s (RPCS3's patch manager downloads it); no patches\n", yml.c_str()); return; }
    const std::string key = std::string("PPU-") + hash + ":";
    char line[1024];
    bool in_block = false;
    std::string name, author;
    std::vector<std::string> applied;
    int writes = 0, bad = 0;
    auto flush = [&]() {
        if (!name.empty() && writes) {
            fprintf(stderr, "[patch] \"%s\"%s%s%s: %d write(s)%s\n", name.c_str(),
                    author.empty() ? "" : " (", author.c_str(), author.empty() ? "" : ")", writes,
                    bad ? " (some entries of unsupported types skipped)" : "");
            applied.push_back(lower(name));
        }
        writes = bad = 0;
    };
    bool take = false;
    while (fgets(line, sizeof line, f)) {
        std::string l = line;
        while (!l.empty() && (l.back() == '\n' || l.back() == '\r')) l.pop_back();
        if (!l.empty() && l[0] != ' ' && l[0] != '#') {          /* a top-level key */
            if (in_block) { flush(); name.clear(); }
            in_block = l == key;
            continue;
        }
        if (!in_block) continue;
        if (l.size() > 3 && l.compare(0, 3, "  \"") == 0 && l.back() == ':') {   /* a patch's name */
            flush();
            name = trim(l.substr(2, l.size() - 3));
            author.clear();
            take = false;
            for (const auto& w : want) if (w == lower(name)) take = true;
            continue;
        }
        if (!take) continue;
        const size_t ap = l.find("Author:");
        if (ap != std::string::npos) { author = trim(l.substr(ap + 7)); continue; }
        const size_t lb = l.find("- ["), rb = l.find(']');
        if (lb == std::string::npos || rb == std::string::npos || rb < lb) continue;
        std::string body = l.substr(lb + 3, rb - lb - 3);
        std::vector<std::string> parts;
        { size_t a = 0; while (a <= body.size()) { size_t b = body.find(',', a); if (b == std::string::npos) b = body.size();
                                                    parts.push_back(trim(body.substr(a, b - a))); a = b + 1; } }
        if (parts.size() < 3) { bad++; continue; }
        const uint32_t addr = (uint32_t)strtoul(parts[1].c_str(), nullptr, 0);
        if (apply_one(parts[0], addr, parts[2])) writes++; else bad++;
    }
    if (in_block) flush();
    fclose(f);
    for (const auto& w : want) {
        bool done = false;
        for (const auto& a : applied) if (a == w) done = true;
        if (!done) fprintf(stderr, "[patch] \"%s\" not found for this EBOOT (PPU-%s) in %s\n", w.c_str(), hash, yml.c_str());
    }
}

/*
 * PS3_ASPECT=21:9|32:9|<w>:<h>|<ratio>: the game's cameras built for a wider
 * picture, hor+ (the vertical field of view kept, the horizontal one wider).
 * The toolkit's Metal backend renders every target that much wider
 * (anamorphic) and presents the wide image; this is the game's half.
 *
 * Found 2026-10-23: the camera constructor (func_0047F5xx, the store at
 * 0x0047F85C) sets cam+0x2A4 = (float)width / (float)height from two ints it
 * reaches through its module's pointer table (r30 = 0x009B72F0, entries
 * 0x009AF3EC -> 0x009D7218 = 1280 and 0x009AF3F0 -> 0x009D7214 = 720), and
 * SetFov (func_0047D820) keeps tany = tan(fov/2) at cam+0x2AC and
 * tanx = tany * cam+0x2A4 at cam+0x2A8; the projection, the frustum the
 * culling uses and the copies the renderer takes (func_00355290) all come
 * from those. No other code reads the two pointer entries (a scan of every
 * load through that module's table), and the 1280 itself is read by many
 * others (render targets), so the patch repoints the width ENTRY at an int
 * of its own: round(720 * aspect), in the unused gap between the EBOOT's
 * two segments (0x00928168..0x0092FFFF: no segment, below the stacks and
 * the 0x40000000 allocation window). Data only, like the community patches;
 * checked against this EBOOT's bytes first, so another build is left alone.
 */
extern "C" void infamous2_apply_aspect(const char* elf_path)
{
    const char* e = getenv("PS3_ASPECT");
    if (!e || !*e || !vm_base) return;
    double a = 0.0;
    unsigned w = 0, h = 0;
    if (sscanf(e, "%u:%u", &w, &h) == 2 && w && h) a = (double)w / (double)h;
    else if (sscanf(e, "%ux%u", &w, &h) == 2 && w && h) a = (double)w / (double)h;
    else { char* end = nullptr; a = strtod(e, &end); if (!end || *end) a = 0.0; }
    const double k = a / (16.0 / 9.0);
    if (!(k > 1.001 && k <= 4.0)) return;   /* 16:9 (or nonsense: the backend says so) */

    auto rd = [](uint32_t ea) { const uint8_t* p = vm_base + ea;
                                return (uint32_t)p[0] << 24 | (uint32_t)p[1] << 16 | (uint32_t)p[2] << 8 | p[3]; };
    auto wr = [](uint32_t ea, uint32_t v) { uint8_t* p = vm_base + ea;
                                            p[0] = (uint8_t)(v >> 24); p[1] = (uint8_t)(v >> 16); p[2] = (uint8_t)(v >> 8); p[3] = (uint8_t)v; };
    const uint32_t kWidthEntry = 0x009AF3ECu, kHeightEntry = 0x009AF3F0u;
    const uint32_t kWidth = 0x009D7218u, kHeight = 0x009D7214u, kSpare = 0x0092FFF0u;
    char hash[41] = {0};
    if (!ppu_hash(elf_path, hash) || strcmp(hash, "41a7d047c573c78c0307506c9d1b52d15a66e4f5") ||
        rd(kWidthEntry) != kWidth || rd(kHeightEntry) != kHeight || rd(kSpare) != 0) {
        fprintf(stderr, "[patch] PS3_ASPECT=%s: not the EBOOT this was made for (PPU-%s); the game keeps 16:9 "
                        "(the picture will be stretched)\n", e, hash);
        return;
    }
    const uint32_t height = rd(kHeight) ? rd(kHeight) : 720u;
    const uint32_t width = (uint32_t)(height * a + 0.5);
    wr(kSpare, width);
    wr(kWidthEntry, kSpare);
    fprintf(stderr, "[patch] PS3_ASPECT=%s: cameras at %u:%u (aspect %.4f, hor+)\n", e, width, height,
            (double)width / (double)height);
}
