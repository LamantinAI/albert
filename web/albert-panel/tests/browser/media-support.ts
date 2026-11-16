export function recording() {
  const samples = 16000;
  const bytes = Buffer.alloc(44 + samples * 2);
  bytes.write("RIFF");
  bytes.writeUInt32LE(bytes.length - 8, 4);
  bytes.write("WAVEfmt ", 8);
  bytes.writeUInt32LE(16, 16);
  bytes.writeUInt16LE(1, 20);
  bytes.writeUInt16LE(1, 22);
  bytes.writeUInt32LE(8000, 24);
  bytes.writeUInt32LE(16000, 28);
  bytes.writeUInt16LE(2, 32);
  bytes.writeUInt16LE(16, 34);
  bytes.write("data", 36);
  bytes.writeUInt32LE(samples * 2, 40);
  for (let i = 0; i < samples; i++)
    bytes.writeInt16LE(
      Math.round(Math.sin(i * 0.19) * (Math.sin(i / 700) + 1) * 7000),
      44 + i * 2,
    );
  return bytes;
}
export const audio = recording();
export const picture = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLbtAAAAABJRU5ErkJggg==",
  "base64",
);
export const attachments = [
  {
    id: "image-1",
    filename: "Frame.png",
    content_type: "image/png",
    size: picture.length,
  },
  {
    id: "audio-1",
    filename: "Voice note.wav",
    content_type: "audio/wav",
    size: audio.length,
  },
  { id: "video-1", filename: "Clip.mp4", content_type: "video/webm", size: 32 },
];
