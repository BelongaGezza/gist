# GIST Icon Set Design Specification

This document outlines the technical and visual requirements for the **GIST** focused reading application icon set. It covers the core design token definitions, platform-specific adaptive execution parameters, and in-app utility icon frameworks required for cross-platform deployment.

---

## 🎨 1. Core Brand & Typography Tokens

| Token Category | Attribute | Value / Constraint |
| :--- | :--- | :--- |
| **Color Palette** | Primary Dark Background | `#1C1C1E` (Rich Matte Charcoal) |
| | Primary Accent / Action | `#4361EE` (Vivid Indigo) |
| | Surface / Page Contrast | `#F4F4F0` (Paper Cream) |
| | Foreground / Vector Fill | `#FFFFFF` (Stark White) |
| **Grid System** | Vector Canvas Baseline | `512 × 512 px` (Nested within 1024×1024 master) |
| | Open Book Scale Bounds | `380 × 260 px` |
| | Lens Diameter Bounds | `140 px` |
| **Geometry** | Primary Stroke Profile | `16 px` (Solid uniform paths) |
| | Emphasis Node Boost | `24 px` (For structural junctions) |
| | Outer Corner Radius | `12 px` (Approachable modern smoothing) |

---

## 🖥️ 2. Desktop Target Environments

### macOS (Primary Platform)
* **Canvas Size**: 1024 × 1024 px
* **Format**: `.icns` or multi-resolution `.png` collection
* **Canvas Mask Constraints**: Asymmetric 3D squircle container with a true physical footprint.
* **Lighting & Shading**: Soft, diffuse ambient drop shadow (`#000000`, 25% opacity, 8px blur, 4px Y-offset) bleeding outside container bounds.
* **Layout Treatment**: Thick textured cream-paper book tilted at a **9-degree isometric angle**. Magnifying glass handle projects outside the boundary of the book page. Lens features a subtle 15% white linear gradient reflection.

### Windows 11
* **Canvas Size**: 256 × 256 px
* **Format**: `.ico` (Embedded RGB)
* **Canvas Mask Constraints**: Strictly flat, bordered tile aligned to modern desktop grid environments.
* **Lighting & Shading**: Leverages native **Mica material** properties. The desktop background tint faintly bleeds through the dark icon base canvas.
* **Layout Treatment**: Razor-sharp, geometric **2D vector strokes**. High-contrast sharp edges with no realistic material gradients or glass reflections.

### Linux
* **Canvas Size**: Fully scalable canvas
* **Format**: `.svg` (Raw, uncompiled vector paths)
* **Canvas Mask Constraints**: Raw, borderless vector silhouette that adapts cleanly to user-defined system icon themes (e.g., Papirus, Yaru).
* **Lighting & Shading**: Zero shadows, 100% transparent asset canvas.
* **Layout Treatment**: High-contrast **monochrome line art** execution using strict, solid `#FFFFFF` or `#1C1C1E` wireframes triggered by the system light/dark mode profile.

---

## 📱 3. Mobile Target Environments

### iOS
* **Canvas Size**: 1024 × 1024 px
* **Format**: `.png` (Flat layer, no alpha channel)
* **Canvas Mask Constraints**: Standard App Store Squircle grid container system (automatically clipped via the iOS system mask).
* **Lighting & Shading**: Soft inner glow on the background canvas to create depth inside a 2D environment.
* **Layout Treatment**: Book centered directly on the **iOS Golden Ratio Grid** intersection lines over a deep dark-mode indigo background canvas. The magnifying glass lens magnifies the abstract text lines directly beneath it by **1.2× scale**.

### Android
* **Canvas Size**: 512 × 512 px
* **Format**: Dual `.xml` vector source layers
* **Canvas Mask Constraints**: Adaptive Icon architecture allowing the system launcher to dynamically scale, parallax-scroll, or mask the asset into circles, squares, or teardrops.
* **Lighting & Shading**: Flat styling with a subtle material drop shadow layered between the foreground asset and background block.
* **Layout Treatment**: 
  * *Background Layer*: Static solid color block or smooth radial gradient.
  * *Foreground Layer*: Book and magnifying glass combined into a single group, maintaining a mandatory **15% safe-zone margin** on all outer edges.

---

## 🛠️ 4. In-App Vector UI Icon Suite
*All utility icons must be drawn on a custom `24 × 24 px` grid using a unified `1.5 px` vector stroke profile.*

* **Home / Library**: A closed book standing vertically, intertwined with a small magnifying glass positioned over the lower spine binding.
* **Smart Summary (The "Gist" Action)**: A magnifying glass framing three horizontal text highlight bars of descending lengths.
* **Focused Mode**: A book paired with a clean, single-point target dot sitting inside the circular glass lens.
* **Search / Discover**: A magnifying glass where the handle subtly transitions into an open, flowing ribbon bookmark profile.
* **Saved / Archive**: An open book with a clear magnifying glass icon stamped into a traditional hanging bookmark flag.

