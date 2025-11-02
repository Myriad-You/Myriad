# Color Extraction Feature

## Overview

The Myriad frontend includes an automatic color extraction system that analyzes the wallpaper image and applies the dominant colors to various UI elements, creating a cohesive visual experience that adapts to the chosen wallpaper.

## How It Works

### 1. Color Extraction Algorithm

When a wallpaper is loaded, the system:

1. **Scales the image** to 192x108 pixels for performance optimization
2. **Quantizes colors** to 16 levels per RGB channel to reduce noise
3. **Analyzes pixels** to identify unique colors and their frequencies
4. **Filters colors** based on:
   - Minimum frequency: 0.5% of total pixels
   - Minimum saturation: 0.1 (to avoid pure grays)
5. **Ranks colors** by a score combining frequency and saturation: `score = percentage × (1 + saturation)`
6. **Returns the top 5** most significant colors

### 2. UI Application

The extracted colors are applied to:

- **Background Overlay Gradient**: Uses the top 3 colors with varying opacity (95%, 90%, 95%)
- **Animated Mesh Blobs**: Three floating gradient blobs use the top 3 colors
- **Dynamic adaptation**: Colors update when the wallpaper changes

### 3. Caching Strategy

To avoid redundant processing:

- Colors are cached in `sessionStorage` keyed by wallpaper URL
- Cache is checked before extraction on page load
- Cache-busting parameter (`?_cb=timestamp`) ensures fresh image load
- Cache persists only for the current browser session

## Configuration

The color extraction behavior can be tuned via constants in `frontend/src/layouts/Layout.astro`:

```typescript
const COLOR_EXTRACTION_CONFIG = {
  CANVAS_WIDTH: 192,           // Canvas width for processing
  CANVAS_HEIGHT: 108,          // Canvas height for processing
  QUANTIZATION_LEVELS: 16,     // Color quantization (1-255)
  MIN_PERCENTAGE: 0.5,         // Minimum color frequency (%)
  MIN_SATURATION: 0.1,         // Minimum color saturation (0-1)
  TOP_COLORS_COUNT: 5,         // Number of colors to extract
};
```

### Tuning Guidelines

- **CANVAS_WIDTH/HEIGHT**: Larger = more accurate but slower. 192x108 provides good balance.
- **QUANTIZATION_LEVELS**: Lower = fewer unique colors but faster. 16 works well for most images.
- **MIN_PERCENTAGE**: Higher = only very dominant colors. Lower = more variety but potentially noise.
- **MIN_SATURATION**: Higher = only vivid colors. Lower = includes more muted tones.
- **TOP_COLORS_COUNT**: More colors = more variety but potentially overwhelming.

## Debugging

The system logs detailed information to the browser console:

```
[Color Extraction] Loading wallpaper: https://...
[Color Extraction] Analyzed 192x108 canvas
[Color Extraction] Found 847 unique colors from 20736 pixels
[Color Extraction] Top 5 significant colors: [
  "rgb(176,208,224) 2.1% sat:0.21",
  "rgb(192,208,224) 2.0% sat:0.14",
  ...
]
[Color Extraction] Applied overlay gradient
[Color Extraction] Applied blob-1 color: rgb(176,208,224)
[Color Extraction] Cached colors for: https://...
```

## Fallback Behavior

If color extraction fails for any reason:

1. The error is logged to console
2. Default colors (defined in CSS) are used
3. The UI remains functional with the spring-themed color palette

## API Integration

The wallpaper URL is provided by the backend API at `/api/config`:

```json
{
  "ui_config": {
    "wallpaper_url": "https://images.unsplash.com/photo-...",
    "wallpaper_blur": 3
  }
}
```

## Performance Considerations

- **Image loading**: Uses `crossOrigin="Anonymous"` to enable canvas processing
- **Processing time**: Typically <100ms for most images
- **Memory usage**: Minimal - canvas is created temporarily and garbage collected
- **Network**: Cache-busting only on initial load, then cached colors are reused

## Browser Compatibility

Requires:
- Canvas API with 2D context
- `getImageData()` support
- `sessionStorage` API
- ES6+ JavaScript features

Compatible with all modern browsers (Chrome, Firefox, Safari, Edge).

## Future Enhancements

Potential improvements:
- Allow user to disable color extraction
- Provide manual color picker override
- Support for color schemes (light/dark mode adaptation)
- AI-based color harmony suggestions
- Accessibility contrast checking
