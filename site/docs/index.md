---
layout: home

title: The log shipper for log-store

hero:
    name: log-ship
    text: REPLACE
    tagline: Easily watch a log file, parse it with Python, then ship it to your favorite log analytics platform.
    image:
      src: 
    actions:
      - theme: brand
        text: Download
        link: /download
      - theme: alt
        text: Introduction
        link: /intro
      - theme: brand
        text: Getting Started
        link: /running

features:
  - icon: 🛠️
    title: Extensible
    details: With all parsers written in Python, you can easily modify any parser to meet your needs.
  - icon: ⚡️
    title: Fast
    details: Just like log-store, log-ship is written in Rust, so it's fast and safe!
  - icon: ▤
    title: Composable
    details: Each shipping route is composed of an input, transformer, and output.
---

<script>
export default {
    mounted() {
        const els = document.getElementsByClassName("text");
        
        for(const e of els) {
            if(e.innerText === 'REPLACE') {
                e.style.maxWidth = "100%";
                e.parentElement.style.maxWidth = "100%";
                e.innerHTML = 'The log shipper for <a href="https://log-store.com" style="text-decoration: underline">log-store</a>';
            }
        }
    }
}
</script>