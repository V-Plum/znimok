// znimok-ocr-convert — turns a float LSTM model (tessdata_best) into an integer one, the same as
// `lstmtraining --stop_training --convert_to_int` but without Tesseract's training tools (which
// need ICU). ZK-120: integer models halve the helper's memory and time at nearly the same
// accuracy (3.0 % vs 2.8 % CER on tools/ocr-eval).
//
//   znimok-ocr-convert in.traineddata out.traineddata
//
// Uses Tesseract's internal classes; built against the pinned source tree by build.py.

#include "lstmrecognizer.h"
#include "serialis.h"
#include "tessdatamanager.h"

#include <cstdio>
#include <vector>

namespace {
// lstmtraining converts a network whose training is enabled and writes it with training
// temporarily disabled; the state is part of the file and the recogniser relies on it (a model
// written in the permanently disabled state crashes it). `network_` is protected: a subclass.
struct Converter : tesseract::LSTMRecognizer {
  void set_training(tesseract::TrainingState s) { network_->SetEnableTraining(s); }
};
}  // namespace

int main(int argc, char** argv) {
  if (argc != 3) {
    fprintf(stderr, "usage: znimok-ocr-convert in.traineddata out.traineddata\n");
    return 2;
  }
  tesseract::TessdataManager mgr;
  if (!mgr.Init(argv[1])) {
    fprintf(stderr, "cannot read %s\n", argv[1]);
    return 1;
  }
  tesseract::TFile in;
  Converter rec;
  if (!mgr.GetComponent(tesseract::TESSDATA_LSTM, &in) || !rec.DeSerialize(&mgr, &in)) {
    fprintf(stderr, "%s has no LSTM model\n", argv[1]);
    return 1;
  }
  rec.set_training(tesseract::TS_ENABLED);
  rec.ConvertToInt();
  rec.set_training(tesseract::TS_TEMP_DISABLE);
  std::vector<char> data;
  tesseract::TFile out;
  out.OpenWrite(&data);
  if (!rec.Serialize(&mgr, &out)) {
    fprintf(stderr, "cannot serialise the model\n");
    return 1;
  }
  mgr.OverwriteEntry(tesseract::TESSDATA_LSTM, data.data(), static_cast<int>(data.size()));
  if (!mgr.SaveFile(argv[2], tesseract::SaveDataToFile)) {
    fprintf(stderr, "cannot write %s\n", argv[2]);
    return 1;
  }
  return 0;
}
