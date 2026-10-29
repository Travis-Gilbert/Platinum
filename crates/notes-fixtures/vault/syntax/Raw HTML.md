---
tags: [syntax, html]
---
# Raw HTML

<div class="custom-block" id="custom-id">A div with a class and an id.</div>

<span style="color: red">A span with an inline style</span>, <kbd>Ctrl</kbd>+<kbd>E</kbd>, <mark>mark</mark>, H<sub>2</sub>O and x<sup>2</sup>.

<details>
<summary>A details element</summary>

Markdown inside is rendered when a blank line separates it from the tags: **bold**.

</details>

<div>**No Markdown inside a block of HTML**</div>

<img src="../attachments/pixel.png" width="64" alt="An img tag">

<br>

<audio controls src="../attachments/tone.mp3"></audio>

Removed by sanitising:

<script>document.title = "script ran";</script>

<iframe src="https://example.com/frame" width="200" height="100"></iframe>

<a href="javascript:alert(1)">A javascript link</a>

<style>body { background: red; }</style>

<img src="x" onerror="alert(1)">

<!-- An HTML comment -->
