export function canChooseMediaQuality(file: Pick<File, "name">): boolean {
  return /\.(jpe?g|png|webp|mp4|mov|m4v|webm|mkv)$/i.test(file.name);
}
