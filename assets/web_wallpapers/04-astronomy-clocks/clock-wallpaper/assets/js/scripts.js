function digitalClock() {
    const now = new Date();
    const daysOfWeek = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
    let h = now.getHours();
    let amOrPm = h < 12 ? "AM" : "PM";
    document.getElementById("h").innerHTML = ((h + 11) % 12 + 1).toString().padStart(2, "0");
    document.getElementById("m").innerHTML = now.getMinutes().toString().padStart(2, "0");
    document.getElementById("t").innerHTML = amOrPm;
    document.getElementById("hours").innerHTML = ((h + 11) % 12 + 1).toString().padStart(2, "0");
    document.getElementById("minutes").innerHTML = now.getMinutes().toString().padStart(2, "0");
    document.getElementById("seconds").innerHTML = now.getSeconds().toString().padStart(2, "0");
    document.getElementById("time").innerHTML = amOrPm;
    document.getElementById("day").innerHTML = daysOfWeek[now.getDay()];
    document.getElementById("date").innerHTML = now.getDate().toString().padStart(2, "0");
    document.getElementById("month").innerHTML = (now.getMonth() + 1).toString().padStart(2, "0");
    document.getElementById("year").innerHTML = now.getFullYear();
  }
  setInterval(digitalClock, 1);