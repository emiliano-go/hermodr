// Sticker library state: a version counter the picker watches so a favorite or
// recent change synced from the phone redraws without reopening the picker.
class StickerState {
  version = $state(0);

  touch() {
    this.version += 1;
  }
}

export const stickers = new StickerState();
