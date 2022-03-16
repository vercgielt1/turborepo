import { Avatar } from "./Avatar";

const team = {
  jaredpalmer: {
    name: "Jared Palmer",
    twitterUsername: "jaredpalmer",
    picture: "/images/people/jaredpalmer_headshot.jpeg",
  },
  gaspargarcia_: {
    name: "Gaspar Garcia",
    twitterUsername: "gaspargarcia_",
    picture: "/images/people/gaspargarcia_.jpeg",
  },
  becca__z: {
    name: "Becca Z.",
    twitterUsername: "becca__z",
    picture: "/images/people/becca__z.jpeg",
  },
  gsoltis: {
    name: "Greg Soltis",
    twitterUsername: "gsoltis",
    picture: "/images/people/gsoltis.jpeg",
  },
};

export function Authors({ authors }) {
  return (
    <div className="grid max-w-screen-md gap-4 mt-6 px-6 sm:grid-cols-2 md:grid-cols-4">
      {authors.map((username) =>
        !!team[username] ? (
          <Avatar key={username} {...team[username]} />
        ) : (
          console.warning("no author found for", username) || null
        )
      )}
    </div>
  );
}
