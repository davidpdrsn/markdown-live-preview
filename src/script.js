async function reload() {
    var response = await fetch('/markdown')
    var text = await response.text()
    var el = document.querySelector("#contents")
    el.innerHTML = text
}

document.addEventListener('DOMContentLoaded', (ev) => {
    reload()
})

var socket = new WebSocket("ws://localhost:{port}/ws");

socket.addEventListener('message', (ev) => {
    reload()
});
