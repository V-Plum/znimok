// znimok-ocr — Znimok's on-device text recognition helper for Windows (ZK-120).
//
// A short-lived child process: Znimok starts it when text must be recognised, sends pictures
// over stdin, reads results from stdout, and the helper exits by itself after a few idle
// seconds, so no memory is held between uses (owner, 29.09: RAM matters more than disk). The
// parent also caps its memory with a job object.
//
// It never decodes image files: it gets 8-bit grayscale pixels, so no PNG/JPEG/TIFF code is in
// it. Models: Tesseract tessdata_best ukr + eng, loaded in ~0.1 s.
//
// Memory: layout is found on the picture as is; only lines of small text are scaled ×2, one
// line at a time — never the whole picture (a 2560×1440 screen at ×2 alone costs ~100 MB).
//
// Layout (ZK-308): a screenshot of a web page or an app is not a scanned page — colour panels,
// photos, light text on dark. Tesseract's page layout on the whole picture took a hero banner
// for one «text line» 800 px tall and lost every light-on-dark line. So: layout and recognition
// run twice, on the picture as it is and on its negative (light text on dark becomes dark on
// light), boxes that cannot be a line of text are dropped, and the two sets of lines are merged
// (the better reading of the same place wins).
//
// Protocol (little-endian):
//   request:  "ZOCR" u32 version u32 mode u32 width u32 height
//             version 2: then u32 n and n bytes of Tesseract languages ("eng", "ukr+eng"; empty =
//             the mode's default), then width*height gray bytes (version 1: no languages field)
//             mode 0 = text (default ukr+eng), 1 = masking (default eng+ukr: Latin reads better
//             — e-mail, keys, cards), 2 = quit
//   response: u32 length, then UTF-8 JSON:
//             {"lines":[{"text","x","y","w","h","conf","words":[{"text","x","y","w","h","conf"}]}]}
//             or {"error":"..."}
//
// Arguments: --tessdata DIR (default: <exe dir>\tessdata), --idle SECONDS (default 5),
//            --version.

#include <tesseract/baseapi.h>
#include <tesseract/resultiterator.h>
#include <leptonica/allheaders.h>

#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <string>
#include <thread>
#include <vector>

#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#endif

namespace {

constexpr uint32_t kVersionMin = 1, kVersionMax = 2;
// A line of interface text is never taller than this share of the picture (a hero banner, a
// photo or a column that layout took for a line is dropped), nor shorter than this.
constexpr int kMinLine = 6;
// The line height the models read well; headings taller than twice this are scaled down to it,
// and a layout box taller than that may hold several lines — it is laid out again on its own.
constexpr int kLineHeight = 40;
// Lines read with less confidence than this are noise (logos, icons, photo texture read as
// letters): the evaluation's real lines read at 80–97, garbage at 8–45 — the first pass keeps
// from kMinConf; the later passes (the negative, the local threshold, the panels) read photos
// and icons as «words» at 40–75, so they keep from kLaterConf, and a line of three letters or
// fewer must be surer than kShortConf and no taller than a heading.
constexpr float kMinConf = 35.0f, kLaterConf = 50.0f, kShortConf = 80.0f;
constexpr uint32_t kMaxSide = 16384;
constexpr uint64_t kMaxPixels = 64ull << 20;
// Lines lower than this (pixels) are scaled ×2 before recognition — what the evaluation
// (tools/ocr-eval) measured on 13–20 px interface text.
constexpr int kSmallLine = 40;

std::atomic<int64_t> g_last_ms{0};
std::atomic<bool> g_busy{false};

int64_t now_ms() {
  using namespace std::chrono;
  return duration_cast<milliseconds>(steady_clock::now().time_since_epoch()).count();
}

bool read_all(void* dst, size_t n) {
  auto* p = static_cast<unsigned char*>(dst);
  while (n > 0) {
    size_t got = fread(p, 1, n, stdin);
    if (got == 0) return false;
    p += got;
    n -= got;
  }
  return true;
}

uint32_t le32(const unsigned char* b) {
  return uint32_t(b[0]) | uint32_t(b[1]) << 8 | uint32_t(b[2]) << 16 | uint32_t(b[3]) << 24;
}

void reply(const std::string& json) {
  uint32_t n = static_cast<uint32_t>(json.size());
  unsigned char h[4] = {uint8_t(n), uint8_t(n >> 8), uint8_t(n >> 16), uint8_t(n >> 24)};
  fwrite(h, 1, 4, stdout);
  fwrite(json.data(), 1, json.size(), stdout);
  fflush(stdout);
}

std::string esc(const char* s) {
  std::string o;
  for (const unsigned char* p = reinterpret_cast<const unsigned char*>(s); *p; ++p) {
    unsigned char c = *p;
    if (c == '"' || c == '\\') {
      o += '\\';
      o += char(c);
    } else if (c < 0x20) {
      char buf[8];
      snprintf(buf, sizeof buf, "\\u%04x", c);
      o += buf;
    } else {
      o += char(c);
    }
  }
  return o;
}

std::string error(const std::string& m) { return "{\"error\":\"" + esc(m.c_str()) + "\"}"; }

struct Gray {
  int w = 0, h = 0;
  std::vector<unsigned char> px;
};

// Scale by a factor with a Catmull-Rom (bicubic) filter, separably: sharp enough to keep
// colons, dots and «·» that bilinear blurs away (a line at a time: small). Small lines go up
// ×2; headings taller than the models like (ZK-308) come down to kLineHeight.
float cubic(float t) {
  t = std::fabs(t);
  if (t < 1) return 1.5f * t * t * t - 2.5f * t * t + 1;
  if (t < 2) return -0.5f * t * t * t + 2.5f * t * t - 4 * t + 2;
  return 0;
}

Gray rescale(const Gray& src, float k) {
  // Horizontal pass into floats, then vertical into bytes.
  int w = std::max(1, int(std::lround(src.w * k))), h = std::max(1, int(std::lround(src.h * k)));
  std::vector<float> mid(size_t(w) * src.h);
  for (int x = 0; x < w; ++x) {
    float fx = (x + 0.5f) / k - 0.5f;
    int x0 = int(std::floor(fx));
    float wt[4], sum = 0;
    for (int i = 0; i < 4; ++i) sum += wt[i] = cubic(fx - float(x0 - 1 + i));
    for (int y = 0; y < src.h; ++y) {
      float v = 0;
      for (int i = 0; i < 4; ++i) {
        int xx = std::clamp(x0 - 1 + i, 0, src.w - 1);
        v += wt[i] * src.px[size_t(y) * src.w + xx];
      }
      mid[size_t(y) * w + x] = v / sum;
    }
  }
  Gray d;
  d.w = w;
  d.h = h;
  d.px.resize(size_t(w) * h);
  for (int y = 0; y < h; ++y) {
    float fy = (y + 0.5f) / k - 0.5f;
    int y0 = int(std::floor(fy));
    float wt[4], sum = 0;
    for (int i = 0; i < 4; ++i) sum += wt[i] = cubic(fy - float(y0 - 1 + i));
    for (int x = 0; x < w; ++x) {
      float v = 0;
      for (int i = 0; i < 4; ++i) {
        int yy = std::clamp(y0 - 1 + i, 0, src.h - 1);
        v += wt[i] * mid[size_t(yy) * w + x];
      }
      d.px[size_t(y) * w + x] = static_cast<unsigned char>(std::clamp(v / sum + 0.5f, 0.0f, 255.0f));
    }
  }
  return d;
}

Gray crop(const Gray& g, int l, int t, int r, int b) {
  Gray c;
  c.w = r - l;
  c.h = b - t;
  c.px.resize(size_t(c.w) * c.h);
  for (int y = 0; y < c.h; ++y)
    memcpy(&c.px[size_t(y) * c.w], &g.px[size_t(t + y) * g.w + l], size_t(c.w));
  return c;
}

using Box = std::array<int, 4>;  // left, top, right, bottom

// A layout box trimmed to its ink and cut at blank rows: the page layout's box of a heading
// runs into the line below (the crop then reads as another script), and a box of two lines
// becomes two. The background is the box's commonest brightness; a row with a few pixels
// away from it holds ink.
std::vector<Box> ink_rows(const Gray& g, const Box& bx) {
  int l = std::max(0, bx[0]), t = std::max(0, bx[1]), r = std::min(g.w, bx[2]), b = std::min(g.h, bx[3]);
  std::vector<Box> out;
  if (r - l < 2 || b - t < 2) return out;
  int hist[64] = {0};
  for (int y = t; y < b; ++y)
    for (int x = l; x < r; ++x) ++hist[g.px[size_t(y) * g.w + x] >> 2];
  int bg = int(std::max_element(hist, hist + 64) - hist) * 4 + 2;
  const int need = std::max(1, (r - l) / 200);  // pixels of ink a row must hold
  std::vector<bool> ink(size_t(b - t), false);
  for (int y = t; y < b; ++y) {
    int n = 0;
    for (int x = l; x < r && n <= need; ++x)
      if (std::abs(int(g.px[size_t(y) * g.w + x]) - bg) > 40) ++n;
    ink[size_t(y - t)] = n > need;
  }
  // Bands of ink rows; a gap of up to two blank rows (the bar of an «i», a diacritic) stays in.
  int y = 0, n = b - t;
  while (y < n) {
    while (y < n && !ink[size_t(y)]) ++y;
    if (y >= n) break;
    int start = y, last = y;
    while (y < n) {
      if (ink[size_t(y)]) last = y;
      else if (y - last > 2) break;
      ++y;
    }
    if (last - start + 1 >= kMinLine) out.push_back({l, t + start, r, t + last + 1});
  }
  if (out.empty()) out.push_back(bx);
  return out;
}

// Mean brightness of a box: pieces on different backgrounds (a dark button beside white text
// on a banner) must not join — one threshold cannot serve both and one of them is lost.
int brightness(const Gray& g, const Box& b) {
  long long sum = 0, n = 0;
  for (int y = std::max(0, b[1]); y < std::min(g.h, b[3]); ++y)
    for (int x = std::max(0, b[0]); x < std::min(g.w, b[2]); ++x, ++n) sum += g.px[size_t(y) * g.w + x];
  return n ? int(sum / n) : 128;
}

// A line of text does not change its background. Layout on a page with a banner takes a
// heading on the banner together with a white card beside it for one line (the box then
// reads as one, and one of the two is lost to the threshold), so a box is cut where the mean
// brightness of its columns shifts for longer than half a line height.
std::vector<Box> split_by_background(const Gray& g, const Box& bx) {
  int l = std::max(0, bx[0]), t = std::max(0, bx[1]), r = std::min(g.w, bx[2]), b = std::min(g.h, bx[3]);
  int h = b - t, w = r - l;
  std::vector<Box> out;
  if (h <= 0 || w < 4 * h) {
    out.push_back(bx);
    return out;
  }
  std::vector<int> col(w);
  for (int x = 0; x < w; ++x) {
    long long sum = 0;
    for (int y = t; y < b; ++y) sum += g.px[size_t(y) * g.w + l + x];
    col[x] = int(sum / h);
  }
  // Smooth over a line height: letters are thinner than that, a change of panel is not.
  int win = std::max(2, h / 2);
  std::vector<int> sm(w);
  long long acc = 0;
  for (int x = 0; x < w; ++x) {
    acc += col[x];
    if (x >= win) acc -= col[x - win];
    sm[x] = int(acc / std::min(x + 1, win));
  }
  int start = 0, ref = sm[std::min(w - 1, win)];
  for (int x = win; x < w; ++x) {
    if (std::abs(sm[x] - ref) < 48) continue;
    // A shift: does it hold for half a line height?
    int run = 0;
    while (x + run < w && std::abs(sm[x + run] - ref) >= 48) ++run;
    if (run < win) {
      x += run;
      continue;
    }
    if (x - start >= 2 * kMinLine) out.push_back({l + start, t, l + x, b});
    start = x;
    ref = sm[std::min(w - 1, x + win)];
    x += win;
  }
  if (w - start >= 2 * kMinLine) out.push_back({l + start, t, r, b});
  if (out.empty()) out.push_back(bx);
  return out;
}

// Layout often cuts one line into pieces that overlap (a line split around an arrow or a wide
// gap). Pieces at the same height join into one line; pieces further apart than a few line
// heights, or on different backgrounds, stay separate (columns, buttons). Output: top to
// bottom, left to right.
std::vector<Box> rows(const Gray& img, std::vector<Box> in) {
  std::sort(in.begin(), in.end(), [](const Box& a, const Box& b) { return a[1] < b[1]; });
  std::vector<std::vector<Box>> groups;
  for (const Box& b : in) {
    bool placed = false;
    for (auto& g : groups) {
      const Box& f = g.front();
      int overlap = std::min(f[3], b[3]) - std::max(f[1], b[1]);
      int hmin = std::min(f[3] - f[1], b[3] - b[1]);
      if (hmin > 0 && overlap * 2 >= hmin) {
        g.push_back(b);
        placed = true;
        break;
      }
    }
    if (!placed) groups.push_back({b});
  }
  std::vector<Box> out;
  for (auto& g : groups) {
    std::sort(g.begin(), g.end(), [](const Box& a, const Box& b) { return a[0] < b[0]; });
    Box cur = g.front();
    for (size_t i = 1; i < g.size(); ++i) {
      const Box& b = g[i];
      int h = std::max(cur[3] - cur[1], b[3] - b[1]);
      if (b[0] - cur[2] <= 3 * h && std::abs(brightness(img, cur) - brightness(img, b)) < 48) {
        cur = {std::min(cur[0], b[0]), std::min(cur[1], b[1]), std::max(cur[2], b[2]),
               std::max(cur[3], b[3])};
      } else {
        out.push_back(cur);
        cur = b;
      }
    }
    out.push_back(cur);
  }
  std::sort(out.begin(), out.end(), [](const Box& a, const Box& b) {
    return a[1] != b[1] ? a[1] < b[1] : a[0] < b[0];
  });
  return out;
}

// Filled panels: a button, a chip, a badge — a flat rectangle of one colour with a label in it.
// Tesseract's layout takes a filled button for a picture and its label is never a line (the
// label alone reads at 96). The flat pixels (same as the neighbours to the right and below)
// form components; a component the size of a button whose box is mostly flat but not wholly
// (the label is the hole) is a panel. The page's own background is far too big to count.
std::vector<Box> panels(const Gray& g) {
  const int w = g.w, h = g.h;
  std::vector<unsigned char> flat(size_t(w) * h, 0);
  for (int y = 0; y + 1 < h; ++y)
    for (int x = 0; x + 1 < w; ++x) {
      const unsigned char* p = &g.px[size_t(y) * w + x];
      flat[size_t(y) * w + x] = std::abs(int(p[0]) - int(p[1])) <= 3 && std::abs(int(p[0]) - int(p[w])) <= 3;
    }
  std::vector<Box> out;
  std::vector<int> stack;
  const int min_h = 14, max_h = 96, min_w = 40;
  for (int y = 0; y < h; ++y)
    for (int x = 0; x < w; ++x) {
      if (!flat[size_t(y) * w + x]) continue;
      // Flood the component (4-connected), marking as visited.
      int l = x, t = y, r = x, b = y;
      long long n = 0;
      stack.push_back(y * w + x);
      flat[size_t(y) * w + x] = 0;
      while (!stack.empty()) {
        int i = stack.back();
        stack.pop_back();
        int cy = i / w, cx = i % w;
        ++n;
        l = std::min(l, cx);
        r = std::max(r, cx);
        t = std::min(t, cy);
        b = std::max(b, cy);
        const int nb[4] = {i - 1, i + 1, i - w, i + w};
        for (int k = 0; k < 4; ++k) {
          int j = nb[k];
          if (j < 0 || j >= w * h) continue;
          if ((k == 0 && cx == 0) || (k == 1 && cx == w - 1)) continue;
          if (!flat[size_t(j)]) continue;
          flat[size_t(j)] = 0;
          stack.push_back(j);
        }
      }
      int bw = r - l + 1, bh = b - t + 1;
      if (bh < min_h || bh > max_h || bw < min_w || bw > w / 2 || bw < bh) continue;
      long long area = (long long)bw * bh;
      // Mostly flat (a panel, not a frame or a line), with a hole (the label).
      if (n * 100 < area * 45 || n * 100 > area * 97) continue;
      out.push_back({l, t, r + 1, b + 1});
    }
  return out;
}

// Tesseract's thresholding_method: a global Otsu, or Sauvola's local one.
enum class Thresh { Otsu = 0, Sauvola = 2 };

// One recognised line (one pass), boxes in the picture's pixels.
struct Line {
  int x, y, w, h;
  float conf;
  std::string text;   // the words joined with spaces
  std::string words;  // the JSON of the words
  int letters;        // letters and digits (Latin, Cyrillic) in the text
};

// Letters and digits in UTF-8 text: ASCII, Latin with diacritics, Cyrillic. Symbols, arrows
// and the odd glyphs a picture turns into do not count.
int count_letters(const char* s) {
  int n = 0;
  while (*s) {
    unsigned char c = static_cast<unsigned char>(*s);
    uint32_t cp;
    int len;
    if (c < 0x80) {
      cp = c;
      len = 1;
    } else if ((c & 0xE0) == 0xC0) {
      cp = c & 0x1F;
      len = 2;
    } else if ((c & 0xF0) == 0xE0) {
      cp = c & 0x0F;
      len = 3;
    } else {
      cp = c & 0x07;
      len = 4;
    }
    for (int i = 1; i < len && s[i]; ++i) cp = (cp << 6) | (static_cast<unsigned char>(s[i]) & 0x3F);
    s += len;
    if ((cp < 0x80 && isalnum(int(cp))) || (cp >= 0xC0 && cp <= 0x24F && cp != 0xD7 && cp != 0xF7) ||
        (cp >= 0x400 && cp <= 0x4FF))
      ++n;
  }
  return n;
}

Gray invert(const Gray& g) {
  Gray o = g;
  for (auto& p : o.px) p = static_cast<unsigned char>(255 - p);
  return o;
}

// How much of the smaller box the two share.
float overlap_share(const Line& a, const Line& b) {
  int ox = std::min(a.x + a.w, b.x + b.w) - std::max(a.x, b.x);
  int oy = std::min(a.y + a.h, b.y + b.h) - std::max(a.y, b.y);
  if (ox <= 0 || oy <= 0) return 0;
  int small = std::min(a.w * a.h, b.w * b.h);
  return small > 0 ? float(ox) * oy / small : 0;
}

class Engine {
 public:
  explicit Engine(std::string tessdata) : tessdata_(std::move(tessdata)) {}

  bool use(uint32_t mode, std::string langs, std::string& err) {
    if (langs.empty()) langs = mode == 1 ? "eng+ukr" : "ukr+eng";
    if (api_ && langs == langs_) return true;
    api_ = std::make_unique<tesseract::TessBaseAPI>();
    if (api_->Init(tessdata_.c_str(), langs.c_str(), tesseract::OEM_LSTM_ONLY) != 0) {
      api_.reset();
      err = std::string("cannot load models ") + langs + " from " + tessdata_;
      return false;
    }
    api_->SetVariable("debug_file", "NUL");
    langs_ = langs;
    return true;
  }

  std::string recognize(const Gray& g) {
    const bool timing = getenv("ZNIMOK_OCR_TIMING") != nullptr;
    const int64_t t_start = now_ms();
    // The page layout of the picture as it is (global threshold; it reads a line of code
    // whole), then its sparse-text layout (the headings, cards and labels the page layout
    // takes for pictures — 17 of 19 phrases of a page against 12), then its negative (light
    // text on a dark or coloured panel), then with a local threshold (lines on cards and panels
    // that a global one melts into the background), then the filled buttons. Each later pass
    // reads only what the earlier ones did not.
    using tesseract::PSM_AUTO;
    using tesseract::PSM_SPARSE_TEXT;
    std::vector<Line> lines = pass(g, Thresh::Otsu, PSM_AUTO, {});
    lines.erase(std::remove_if(lines.begin(), lines.end(),
                               [](const Line& l) { return l.conf < kMinConf; }),
                lines.end());
    const int64_t t_first = now_ms();
    const bool debug = getenv("ZNIMOK_OCR_DEBUG") != nullptr;
    auto trace = [&](const char* tag, const std::vector<Line>& ls) {
      if (!debug) return;
      for (const Line& l : ls)
        fprintf(stderr, "  %s (%d,%d %dx%d) %.1f %s\n", tag, l.x, l.y, l.w, l.h, l.conf, l.text.c_str());
    };
    trace("otsu", lines);
    auto merge = [&](std::vector<Line> more) {
      for (Line& l : more) {
        if (l.conf < kLaterConf) continue;
        bool taken = false;
        for (Line& k : lines) {
          if (overlap_share(l, k) < 0.5f) continue;
          taken = true;
          if (l.conf > k.conf + 15.0f) k = l;  // the clearly surer reading of the same place
          break;
        }
        if (!taken) lines.push_back(std::move(l));
      }
    };
    std::vector<Line> sparse = pass(g, Thresh::Otsu, PSM_SPARSE_TEXT, lines);
    trace("sparse", sparse);
    merge(std::move(sparse));
    std::vector<Line> neg = pass(invert(g), Thresh::Otsu, PSM_SPARSE_TEXT, lines);
    trace("neg", neg);
    merge(std::move(neg));
    std::vector<Line> loc = pass(g, Thresh::Sauvola, PSM_SPARSE_TEXT, lines);
    trace("sauvola", loc);
    merge(std::move(loc));
    // Filled buttons and chips, each read inside its own edge.
    std::vector<Box> boxes = panels(g);
    boxes.erase(std::remove_if(boxes.begin(), boxes.end(),
                               [&](const Box& b) { return covered(b, lines); }),
                boxes.end());
    std::vector<Line> labels;
    read_boxes(g, boxes, false, true, labels);
    trace("panel", labels);
    merge(std::move(labels));
    // Noise: unsure readings, «lines» without a letter or digit, a letter or three read off an
    // icon or a photo.
    lines.erase(std::remove_if(lines.begin(), lines.end(),
                               [](const Line& l) {
                                 if (l.letters == 0) return true;
                                 return l.letters <= 3 && (l.conf < kShortConf || l.h > 2 * kLineHeight);
                               }),
                lines.end());
    std::sort(lines.begin(), lines.end(), [](const Line& a, const Line& b) {
      return a.y != b.y ? a.y < b.y : a.x < b.x;
    });
    std::string out = "{\"lines\":[";
    for (size_t i = 0; i < lines.size(); ++i) {
      const Line& l = lines[i];
      char head[160];
      snprintf(head, sizeof head, "%s{\"x\":%d,\"y\":%d,\"w\":%d,\"h\":%d,\"conf\":%.1f,",
               i ? "," : "", l.x, l.y, l.w, l.h, l.conf);
      out += head;
      out += "\"text\":\"" + esc(l.text.c_str()) + "\",\"words\":[" + l.words + "]}";
    }
    if (timing)
      fprintf(stderr, "znimok-ocr: %zu lines, first pass %lld ms, later passes %lld ms\n",
              lines.size(), (long long)(t_first - t_start), (long long)(now_ms() - t_first));
    return out + "]}";
  }

 private:
  // Tesseract's text-line boxes of a picture that can be lines of interface text, each cut
  // where its background changes. `max_line` — taller is a banner, a photo, a column.
  std::vector<Box> text_lines(const Gray& g, Thresh th, tesseract::PageSegMode psm, int max_line) {
    api_->SetImage(g.px.data(), g.w, g.h, 1, g.w);
    api_->SetSourceResolution(70);
    api_->SetPageSegMode(psm);
    api_->SetVariable("thresholding_method", th == Thresh::Sauvola ? "2" : "0");
    std::vector<Box> boxes;
    std::unique_ptr<tesseract::PageIterator> it(api_->AnalyseLayout());
    if (it) {
      do {
        int l, t, r, b;
        if (!it->BoundingBox(tesseract::RIL_TEXTLINE, &l, &t, &r, &b)) continue;
        int w = r - l, h = b - t;
        // A sliver; a vertical strip.
        if (h < kMinLine || h > max_line || w < 2 * kMinLine || w * 2 < h) continue;
        for (const Box& part : split_by_background(g, {l, t, r, b})) boxes.push_back(part);
      } while (it->Next(tesseract::RIL_TEXTLINE));
    }
    return boxes;
  }

  // Whether a box lies where `done` already reads a line well.
  static bool covered(const Box& b, const std::vector<Line>& done) {
    Line l{b[0], b[1], b[2] - b[0], b[3] - b[1], 0, "", "", 0};
    for (const Line& d : done)
      if (d.conf >= 70 && overlap_share(l, d) >= 0.5f) return true;
    return false;
  }

  // Layout: text-line boxes that can be lines of interface text. The layout of a whole page
  // cuts a banner coarsely (two heading lines in one box, a row of buttons with a card); a box
  // taller than two line heights is laid out again on its own, which reads it line by line.
  std::vector<Box> layout(const Gray& g, Thresh th, tesseract::PageSegMode psm) {
    const int max_line = std::max(96, g.h / 10);
    std::vector<Box> boxes;
    for (const Box& bx : text_lines(g, th, psm, max_line)) {
      int h = bx[3] - bx[1];
      if (h <= 2 * kLineHeight) {
        boxes.push_back(bx);
        continue;
      }
      int pad = h / 4;
      int l = std::max(0, bx[0] - pad), t = std::max(0, bx[1] - pad);
      int r = std::min(g.w, bx[2] + pad), b = std::min(g.h, bx[3] + pad);
      std::vector<Box> inner = text_lines(crop(g, l, t, r, b), th, psm, max_line);
      if (inner.empty()) {
        boxes.push_back(bx);
        continue;
      }
      for (Box in : inner) boxes.push_back({in[0] + l, in[1] + t, in[2] + l, in[3] + t});
    }
    std::vector<Box> trimmed;
    for (const Box& bx : boxes)
      for (const Box& part : ink_rows(g, bx)) trimmed.push_back(part);
    boxes = rows(g, std::move(trimmed));
    if (getenv("ZNIMOK_OCR_DEBUG"))
      for (const Box& b : boxes)
        fprintf(stderr, "  box %d,%d %dx%d\n", b[0], b[1], b[2] - b[0], b[3] - b[1]);
    return boxes;
  }

  // One pass: layout, then each line recognised on its own (small ones scaled ×2, headings
  // scaled down), skipping the places `done` already reads well — the same line again, slowly.
  std::vector<Line> pass(const Gray& g, Thresh th, tesseract::PageSegMode psm,
                         const std::vector<Line>& done) {
    std::vector<Line> out;
    std::vector<Box> boxes = layout(g, th, psm);
    const bool first = done.empty();
    if (!first)
      boxes.erase(std::remove_if(boxes.begin(), boxes.end(),
                                 [&](const Box& b) { return covered(b, done); }),
                  boxes.end());
    // Nothing found (a tiny picture): treat it all as one block — the first pass only.
    bool whole = boxes.empty() && first;
    if (whole) boxes.push_back({0, 0, g.w, g.h});
    read_boxes(g, boxes, whole, false, out);
    return out;
  }

  // The lines of `boxes`, each recognised on its own, appended to `out`. `tight`: the box is a
  // panel — read inside its edge, with no room around (the edge and what lies outside would
  // set the threshold wrong).
  void read_boxes(const Gray& g, const std::vector<Box>& boxes, bool whole, bool tight,
                  std::vector<Line>& out) {
    api_->SetVariable("thresholding_method", "0");  // the crops as the evaluation read them
    for (auto& bx : boxes) {
      // Layout leaves a line's final «:» or «.» outside its box: a line height of room on the
      // sides (other columns are at least three line heights away, see rows()).
      int lh = bx[3] - bx[1];
      int pad = tight ? -2 : std::max(2, lh / 4), side = tight ? -2 : std::max(4, lh);
      int l = std::max(0, bx[0] - side), t = std::max(0, bx[1] - pad);
      int r = std::min(g.w, bx[2] + side), b = std::min(g.h, bx[3] + pad);
      if (r - l < 2 || b - t < 2) continue;
      Gray part = crop(g, l, t, r, b);
      float k = lh < kSmallLine ? 2.0f : lh > 2 * kLineHeight ? float(kLineHeight) / lh : 1.0f;
      if (k != 1.0f) part = rescale(part, k);
      api_->SetImage(part.px.data(), part.w, part.h, 1, part.w);
      api_->SetSourceResolution(70);  // as the evaluation saw it; higher drops dots as noise
      api_->SetPageSegMode(whole ? tesseract::PSM_SINGLE_BLOCK : tesseract::PSM_SINGLE_LINE);
      if (api_->Recognize(nullptr) != 0) continue;
      std::unique_ptr<tesseract::ResultIterator> ri(api_->GetIterator());
      if (!ri) continue;
      // One output line per recognised text line (a "block" may hold several).
      std::string words, text;
      int lx0 = 1 << 30, ly0 = 1 << 30, lx1 = -1, ly1 = -1;
      float conf_sum = 0;
      int nwords = 0;
      int letters = 0;
      auto flush = [&]() {
        if (nwords == 0) return;
        out.push_back(Line{lx0, ly0, lx1 - lx0, ly1 - ly0, conf_sum / nwords, text, words, letters});
        words.clear();
        text.clear();
        lx0 = ly0 = 1 << 30;
        lx1 = ly1 = -1;
        conf_sum = 0;
        nwords = 0;
        letters = 0;
      };
      do {
        std::unique_ptr<char[]> w(ri->GetUTF8Text(tesseract::RIL_WORD));
        if (!w || !*w.get()) continue;
        int a, c, e, f;
        ri->BoundingBox(tesseract::RIL_WORD, &a, &c, &e, &f);
        int x0 = l + int(std::floor(a / k)), y0 = t + int(std::floor(c / k));
        int x1 = l + int(std::ceil(e / k)), y1 = t + int(std::ceil(f / k));
        if (strcmp(w.get(), "|") == 0) continue;  // a button's or a card's border, not text
        float cf = ri->Confidence(tesseract::RIL_WORD);
        char head[128];
        snprintf(head, sizeof head, "%s{\"x\":%d,\"y\":%d,\"w\":%d,\"h\":%d,\"conf\":%.1f,",
                 nwords ? "," : "", x0, y0, x1 - x0, y1 - y0, cf);
        words += head;
        words += "\"text\":\"" + esc(w.get()) + "\"}";
        if (nwords) text += ' ';
        text += w.get();
        letters += count_letters(w.get());
        lx0 = std::min(lx0, x0);
        ly0 = std::min(ly0, y0);
        lx1 = std::max(lx1, x1);
        ly1 = std::max(ly1, y1);
        conf_sum += cf;
        ++nwords;
        if (ri->IsAtFinalElement(tesseract::RIL_TEXTLINE, tesseract::RIL_WORD)) flush();
      } while (ri->Next(tesseract::RIL_WORD));
      flush();
    }
    api_->Clear();
  }

  std::string tessdata_;
  std::unique_ptr<tesseract::TessBaseAPI> api_;
  std::string langs_;
};

std::string exe_dir() {
#ifdef _WIN32
  wchar_t buf[MAX_PATH];
  DWORD n = GetModuleFileNameW(nullptr, buf, MAX_PATH);
  std::wstring p(buf, n);
  p = p.substr(0, p.find_last_of(L"\\/"));
  int len = WideCharToMultiByte(CP_UTF8, 0, p.c_str(), -1, nullptr, 0, nullptr, nullptr);
  std::string s(size_t(len > 0 ? len - 1 : 0), '\0');
  WideCharToMultiByte(CP_UTF8, 0, p.c_str(), -1, s.data(), len, nullptr, nullptr);
  return s;
#else
  return ".";
#endif
}

}  // namespace

int main(int argc, char** argv) {
  std::string tessdata = exe_dir() + "/tessdata";
  int idle = 5;
  for (int i = 1; i < argc; ++i) {
    std::string a = argv[i];
    if (a == "--tessdata" && i + 1 < argc) {
      tessdata = argv[++i];
    } else if (a == "--idle" && i + 1 < argc) {
      idle = std::max(1, atoi(argv[++i]));
    } else if (a == "--version") {
      printf("znimok-ocr 0.2.0, tesseract %s, %s\n", tesseract::TessBaseAPI::Version(),
             getLeptonicaVersion());
      return 0;
    }
  }
  // Leptonica's messages (missing codecs it never needs here) would only fill stderr.
  setMsgSeverity(L_SEVERITY_NONE);
#ifdef _WIN32
  _setmode(_fileno(stdin), _O_BINARY);
  _setmode(_fileno(stdout), _O_BINARY);
#endif
  // Exit after `idle` seconds without work: nothing stays in memory between uses.
  g_last_ms = now_ms();
  std::thread([idle] {
    for (;;) {
      std::this_thread::sleep_for(std::chrono::milliseconds(250));
      if (!g_busy && now_ms() - g_last_ms > int64_t(idle) * 1000) std::_Exit(0);
    }
  }).detach();

  Engine engine(tessdata);
  for (;;) {
    unsigned char h[20];
    if (!read_all(h, sizeof h)) return 0;
    g_busy = true;
    uint32_t version = le32(h + 4);
    if (memcmp(h, "ZOCR", 4) != 0 || version < kVersionMin || version > kVersionMax) {
      reply(error("bad request header"));
      return 2;
    }
    uint32_t mode = le32(h + 8), w = le32(h + 12), hh = le32(h + 16);
    std::string langs;
    if (version >= 2) {
      unsigned char nb[4];
      if (!read_all(nb, 4)) return 0;
      uint32_t n = le32(nb);
      if (n > 64) {
        reply(error("bad languages"));
        return 2;
      }
      langs.resize(n);
      if (n && !read_all(langs.data(), n)) return 0;
      for (char c : langs)
        if (!(isalnum(static_cast<unsigned char>(c)) || c == '+' || c == '_')) {
          reply(error("bad languages"));
          return 2;
        }
    }
    if (mode == 2) return 0;
    if (w == 0 || hh == 0 || w > kMaxSide || hh > kMaxSide || uint64_t(w) * hh > kMaxPixels) {
      reply(error("picture size out of bounds"));
      return 2;
    }
    Gray g;
    g.w = int(w);
    g.h = int(hh);
    g.px.resize(size_t(w) * hh);
    if (!read_all(g.px.data(), g.px.size())) return 0;
    std::string err;
    if (mode > 1) {
      reply(error("unknown mode"));
    } else if (!engine.use(mode, langs, err)) {
      reply(error(err));
    } else {
      reply(engine.recognize(g));
    }
    g_last_ms = now_ms();
    g_busy = false;
  }
}
