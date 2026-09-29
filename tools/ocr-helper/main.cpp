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
// Protocol (little-endian):
//   request:  "ZOCR" u32 version=1 u32 mode u32 width u32 height, then width*height gray bytes
//             mode 0 = text (ukr+eng), 1 = masking (eng+ukr: Latin reads better — e-mail,
//             keys, cards), 2 = quit
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

constexpr uint32_t kVersion = 1;
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

// Scale up by an integer factor with a Catmull-Rom (bicubic) filter, separably: sharp enough
// to keep colons, dots and «·» that bilinear blurs away (a line at a time: small).
float cubic(float t) {
  t = std::fabs(t);
  if (t < 1) return 1.5f * t * t * t - 2.5f * t * t + 1;
  if (t < 2) return -0.5f * t * t * t + 2.5f * t * t - 4 * t + 2;
  return 0;
}

Gray scale_up(const Gray& src, int k) {
  // Horizontal pass into floats, then vertical into bytes.
  int w = src.w * k, h = src.h * k;
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

// Layout often cuts one line into pieces that overlap (a line split around an arrow or a wide
// gap). Pieces at the same height join into one line; pieces further apart than a few line
// heights stay separate (columns). Output: top to bottom, left to right.
std::vector<Box> rows(std::vector<Box> in) {
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
      if (b[0] - cur[2] <= 3 * h) {
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

class Engine {
 public:
  explicit Engine(std::string tessdata) : tessdata_(std::move(tessdata)) {}

  bool use(uint32_t mode, std::string& err) {
    if (api_ && mode == mode_) return true;
    const char* langs = mode == 1 ? "eng+ukr" : "ukr+eng";
    api_ = std::make_unique<tesseract::TessBaseAPI>();
    if (api_->Init(tessdata_.c_str(), langs, tesseract::OEM_LSTM_ONLY) != 0) {
      api_.reset();
      err = std::string("cannot load models ") + langs + " from " + tessdata_;
      return false;
    }
    api_->SetVariable("debug_file", "NUL");
    mode_ = mode;
    return true;
  }

  std::string recognize(const Gray& g) {
    const bool timing = getenv("ZNIMOK_OCR_TIMING") != nullptr;
    const int64_t t_start = now_ms();
    std::string out = "{\"lines\":[";
    bool first_line = true;
    // Layout on the picture as it is.
    api_->SetImage(g.px.data(), g.w, g.h, 1, g.w);
    api_->SetSourceResolution(70);
    api_->SetPageSegMode(tesseract::PSM_AUTO);
    std::vector<std::array<int, 4>> boxes;
    {
      std::unique_ptr<tesseract::PageIterator> it(api_->AnalyseLayout());
      if (it) {
        do {
          int l, t, r, b;
          if (it->BoundingBox(tesseract::RIL_TEXTLINE, &l, &t, &r, &b)) boxes.push_back({l, t, r, b});
        } while (it->Next(tesseract::RIL_TEXTLINE));
      }
    }
    boxes = rows(std::move(boxes));
    const int64_t t_layout = now_ms();
    // Nothing found (a tiny picture): treat it all as one block.
    bool whole = boxes.empty();
    if (whole) boxes.push_back({0, 0, g.w, g.h});
    for (auto& bx : boxes) {
      // Layout leaves a line's final «:» or «.» outside its box: a line height of room on the
      // sides (other columns are at least three line heights away, see rows()).
      int lh = bx[3] - bx[1];
      int pad = std::max(2, lh / 4), side = std::max(4, lh);
      int l = std::max(0, bx[0] - side), t = std::max(0, bx[1] - pad);
      int r = std::min(g.w, bx[2] + side), b = std::min(g.h, bx[3] + pad);
      if (r - l < 2 || b - t < 2) continue;
      Gray part = crop(g, l, t, r, b);
      int k = (bx[3] - bx[1]) < kSmallLine ? 2 : 1;
      if (k != 1) part = scale_up(part, k);
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
      auto flush = [&]() {
        if (nwords == 0) return;
        char head[160];
        snprintf(head, sizeof head, "%s{\"x\":%d,\"y\":%d,\"w\":%d,\"h\":%d,\"conf\":%.1f,",
                 first_line ? "" : ",", lx0, ly0, lx1 - lx0, ly1 - ly0, conf_sum / nwords);
        out += head;
        out += "\"text\":\"" + esc(text.c_str()) + "\",\"words\":[" + words + "]}";
        first_line = false;
        words.clear();
        text.clear();
        lx0 = ly0 = 1 << 30;
        lx1 = ly1 = -1;
        conf_sum = 0;
        nwords = 0;
      };
      do {
        std::unique_ptr<char[]> w(ri->GetUTF8Text(tesseract::RIL_WORD));
        if (!w || !*w.get()) continue;
        int a, c, e, f;
        ri->BoundingBox(tesseract::RIL_WORD, &a, &c, &e, &f);
        int x0 = l + a / k, y0 = t + c / k, x1 = l + (e + k - 1) / k, y1 = t + (f + k - 1) / k;
        float cf = ri->Confidence(tesseract::RIL_WORD);
        char head[128];
        snprintf(head, sizeof head, "%s{\"x\":%d,\"y\":%d,\"w\":%d,\"h\":%d,\"conf\":%.1f,",
                 nwords ? "," : "", x0, y0, x1 - x0, y1 - y0, cf);
        words += head;
        words += "\"text\":\"" + esc(w.get()) + "\"}";
        if (nwords) text += ' ';
        text += w.get();
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
    if (timing)
      fprintf(stderr, "znimok-ocr: %zu lines, layout %lld ms, recognition %lld ms\n", boxes.size(),
              (long long)(t_layout - t_start), (long long)(now_ms() - t_layout));
    return out + "]}";
  }

 private:
  std::string tessdata_;
  std::unique_ptr<tesseract::TessBaseAPI> api_;
  uint32_t mode_ = ~0u;
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
      printf("znimok-ocr 0.1.0, tesseract %s, %s\n", tesseract::TessBaseAPI::Version(),
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
    if (memcmp(h, "ZOCR", 4) != 0 || le32(h + 4) != kVersion) {
      reply(error("bad request header"));
      return 2;
    }
    uint32_t mode = le32(h + 8), w = le32(h + 12), hh = le32(h + 16);
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
    } else if (!engine.use(mode, err)) {
      reply(error(err));
    } else {
      reply(engine.recognize(g));
    }
    g_last_ms = now_ms();
    g_busy = false;
  }
}
